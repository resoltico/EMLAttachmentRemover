//! MIME control grammar; shared immutable syntax, never policy eligibility.
use crate::{PreparationError, Refusal, error};
use base64::Engine;

#[derive(Clone, Debug)]
pub(crate) struct Parameter {
    pub name: Vec<u8>,
    pub value: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub(crate) struct Structured {
    pub primary: String,
    pub parameters: Vec<Parameter>,
}
fn malformed() -> PreparationError {
    error(
        Refusal::Malformed,
        "Invalid interpreted MIME control grammar",
    )
}
fn limit() -> PreparationError {
    error(
        Refusal::LimitExceeded,
        "MIME control resource limit exceeded",
    )
}
fn token(byte: u8) -> bool {
    (33..=126).contains(&byte) && !b"()<>@,;:\\\"/[]?=".contains(&byte)
}
fn cfws(value: &[u8], cursor: &mut usize) -> Result<(), PreparationError> {
    loop {
        while value
            .get(*cursor)
            .is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            *cursor += 1;
        }
        if value.get(*cursor) != Some(&b'(') {
            return Ok(());
        }
        comment(value, cursor)?;
    }
}
fn comment(value: &[u8], cursor: &mut usize) -> Result<(), PreparationError> {
    let mut depth = 1;
    *cursor += 1;
    while depth != 0 {
        let byte = *value.get(*cursor).ok_or_else(malformed)?;
        *cursor += 1;
        match byte {
            b'\\' => {
                if *cursor == value.len() {
                    return Err(malformed());
                }
                *cursor += 1;
            }
            b'(' => {
                depth += 1;
                if depth > 32 {
                    return Err(limit());
                }
            }
            b')' => depth -= 1,
            _ => (),
        }
    }
    Ok(())
}
fn unquoted(value: &[u8]) -> Result<Vec<u8>, PreparationError> {
    let mut cursor = 0;
    let mut output = Vec::new();
    let mut whitespace = Vec::new();
    let mut has_comment = false;
    while cursor < value.len() {
        match value[cursor] {
            b' ' | b'\t' => {
                whitespace.push(value[cursor]);
                cursor += 1;
            }
            b'(' => {
                comment(value, &mut cursor)?;
                has_comment = true;
            }
            b')' => return Err(malformed()),
            byte => {
                if !output.is_empty() {
                    if !whitespace.is_empty() {
                        output.extend_from_slice(&whitespace);
                    } else if has_comment {
                        output.push(b' ');
                    }
                }
                whitespace.clear();
                has_comment = false;
                output.push(byte);
                cursor += 1;
            }
        }
    }
    // Only outside CFWS is discarded; internal literal whitespace is preserved.
    Ok(output)
}
fn read_token<'a>(value: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], PreparationError> {
    cfws(value, cursor)?;
    let start = *cursor;
    while value.get(*cursor).is_some_and(|b| token(*b)) {
        *cursor += 1;
    }
    if *cursor == start {
        return Err(malformed());
    }
    Ok(&value[start..*cursor])
}

pub(crate) fn structured(
    value: &[u8],
    media: bool,
    parameter_total: &mut usize,
) -> Result<Structured, PreparationError> {
    let mut cursor = 0;
    let first = read_token(value, &mut cursor)?;
    if media && first.len() > 127 {
        return Err(limit());
    }
    let primary = if media {
        cfws(value, &mut cursor)?;
        if value.get(cursor) != Some(&b'/') {
            return Err(malformed());
        }
        cursor += 1;
        let second = read_token(value, &mut cursor)?;
        if second.len() > 127 {
            return Err(limit());
        }
        format!(
            "{}/{}",
            String::from_utf8(first.to_ascii_lowercase()).map_err(|_| malformed())?,
            String::from_utf8(second.to_ascii_lowercase()).map_err(|_| malformed())?
        )
    } else {
        String::from_utf8(first.to_ascii_lowercase()).map_err(|_| malformed())?
    };
    cfws(value, &mut cursor)?;
    if cursor != value.len() && value[cursor] != b';' {
        return Err(malformed());
    }
    let mut parameters = Vec::new();
    while cursor < value.len() {
        cursor += 1; // required semicolon
        let start = cursor;
        let mut quote = false;
        let mut comments = 0usize;
        while cursor < value.len() {
            let byte = value[cursor];
            if byte == b'\\' && (quote || comments > 0) {
                cursor += 1;
                if cursor < value.len() {
                    cursor += 1;
                }
                continue;
            }
            if byte == b'"' && comments == 0 {
                quote = !quote;
            } else if !quote && byte == b'(' {
                comments += 1;
                if comments > 32 {
                    return Err(limit());
                }
            } else if !quote && byte == b')' && comments > 0 {
                comments -= 1;
            } else if !quote && comments == 0 && byte == b';' {
                break;
            }
            cursor += 1;
        }
        *parameter_total = parameter_total.checked_add(1).ok_or_else(limit)?;
        if parameters.len() >= 256 || *parameter_total > 32_768 {
            return Err(limit());
        }
        parameters.push(parameter(&value[start..cursor]));
    }
    Ok(Structured {
        primary,
        parameters,
    })
}
fn parameter(piece: &[u8]) -> Parameter {
    let mut cursor = 0;
    let Ok(name) = read_token(piece, &mut cursor) else {
        return Parameter {
            name: Vec::new(),
            value: None,
        };
    };
    let value = (|| -> Result<Vec<u8>, PreparationError> {
        cfws(piece, &mut cursor)?;
        if piece.get(cursor) != Some(&b'=') {
            return Err(malformed());
        }
        cursor += 1;
        cfws(piece, &mut cursor)?;
        if piece.get(cursor) == Some(&b'"') {
            cursor += 1;
            let mut value = Vec::new();
            loop {
                let byte = *piece.get(cursor).ok_or_else(malformed)?;
                cursor += 1;
                if byte == b'"' {
                    break;
                }
                if byte == b'\\' {
                    value.push(*piece.get(cursor).ok_or_else(malformed)?);
                    cursor += 1;
                } else {
                    value.push(byte);
                }
            }
            cfws(piece, &mut cursor)?;
            if cursor != piece.len() {
                return Err(malformed());
            }
            Ok(value)
        } else {
            unquoted(&piece[cursor..])
        }
    })()
    .ok();
    Parameter {
        name: name.to_ascii_lowercase(),
        value,
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Form {
    Plain,
    Extended,
    Continuation,
}
fn percent(value: &[u8]) -> Result<Vec<u8>, PreparationError> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < value.len() {
        if value[cursor] == b'%' {
            let hi = *value.get(cursor + 1).ok_or_else(malformed)?;
            let lo = *value.get(cursor + 2).ok_or_else(malformed)?;
            let digit = |b: u8| {
                (b as char)
                    .to_digit(16)
                    .map(|n| n as u8)
                    .ok_or_else(malformed)
            };
            out.push(digit(hi)? * 16 + digit(lo)?);
            cursor += 3;
        } else {
            out.push(value[cursor]);
            cursor += 1;
        }
    }
    Ok(out)
}
pub(crate) struct DecodedParameter {
    pub bytes: Vec<u8>,
    pub charset: Option<Vec<u8>>,
}
/// Returns chosen octets and optional display charset; structural octets are never transcoded.
pub(crate) fn parameter_value(
    parameters: &[Parameter],
    attribute: &[u8],
    structural: bool,
) -> Result<Option<DecodedParameter>, PreparationError> {
    let mut chosen: Option<Form> = None;
    let mut plain: Option<Option<Vec<u8>>> = None;
    let mut segments: Vec<(usize, bool, Option<Vec<u8>>)> = Vec::new();
    let mut duplicate = false;
    for parameter in parameters {
        let Some(suffix) = parameter.name.strip_prefix(attribute) else {
            continue;
        };
        let (form, segment, starred) = if suffix.is_empty() {
            (Form::Plain, 0, false)
        } else if suffix == b"*" {
            (Form::Extended, 0, true)
        } else if let Some(rest) = suffix.strip_prefix(b"*") {
            let starred = rest.ends_with(b"*");
            let digits = if starred {
                &rest[..rest.len() - 1]
            } else {
                rest
            };
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let mut index = 0usize;
            for digit in digits {
                index = index
                    .checked_mul(10)
                    .and_then(|n| n.checked_add((digit - b'0') as usize))
                    .ok_or_else(limit)?;
                if index >= 128 {
                    return Err(limit());
                }
            }
            (Form::Continuation, index, starred)
        } else {
            continue;
        };
        if let Some(previous) = &chosen {
            if previous != &form {
                if structural {
                    return Err(malformed());
                } else {
                    continue;
                }
            }
        } else {
            chosen = Some(form.clone());
        }
        if form == Form::Plain || form == Form::Extended {
            if plain.is_some() {
                duplicate = true;
            } else {
                plain = Some(parameter.value.clone());
            }
        } else if segments.iter().any(|(i, _, _)| *i == segment) {
            duplicate = true;
        } else {
            segments.push((segment, starred, parameter.value.clone()));
        }
    }
    if structural && duplicate {
        return Err(malformed());
    }
    let Some(form) = chosen else {
        return Ok(None);
    };
    let assemble = || -> Result<DecodedParameter, PreparationError> {
        let values = if form == Form::Continuation {
            let mut values = segments.clone();
            values.sort_by_key(|(i, _, _)| *i);
            if values
                .iter()
                .enumerate()
                .any(|(expected, (actual, _, _))| expected != *actual)
            {
                return Err(malformed());
            }
            values
        } else {
            vec![(0, form == Form::Extended, plain.flatten())]
        };
        let mut output = Vec::new();
        let mut charset = None;
        for (index, starred, bytes) in values {
            let mut bytes = bytes.ok_or_else(malformed)?;
            if index == 0 && starred {
                let first = bytes
                    .iter()
                    .position(|b| *b == b'\'')
                    .ok_or_else(malformed)?;
                let second = bytes[first + 1..]
                    .iter()
                    .position(|b| *b == b'\'')
                    .map(|n| n + first + 1)
                    .ok_or_else(malformed)?;
                charset = Some(bytes[..first].to_vec());
                bytes = bytes[second + 1..].to_vec();
            }
            output.extend(if starred { percent(&bytes)? } else { bytes });
        }
        if structural && output.iter().any(|b| !(32..=126).contains(b)) {
            return Err(malformed());
        }
        Ok(DecodedParameter {
            bytes: output,
            charset,
        })
    };
    match assemble() {
        Ok(value) => Ok(Some(value)),
        Err(failure) if structural => Err(failure),
        Err(_) => Ok(None),
    }
}
pub(crate) fn identifier(value: &[u8]) -> Result<Vec<u8>, PreparationError> {
    let mut cursor = 0;
    cfws(value, &mut cursor)?;
    if value.get(cursor) != Some(&b'<') {
        return Err(malformed());
    }
    cursor += 1;
    let start = cursor;
    while value
        .get(cursor)
        .is_some_and(|b| (33..=126).contains(b) && !b"<>".contains(b))
    {
        cursor += 1;
    }
    if cursor == start || value.get(cursor) != Some(&b'>') {
        return Err(malformed());
    }
    let id = value[start..cursor].to_vec();
    cursor += 1;
    cfws(value, &mut cursor)?;
    if cursor != value.len() {
        return Err(malformed());
    }
    Ok(id)
}
pub(crate) fn transfer(value: &[u8]) -> Result<String, PreparationError> {
    let parsed = structured(value, false, &mut 0)?;
    if !parsed.parameters.is_empty() {
        return Err(malformed());
    }
    Ok(parsed.primary)
}
fn encoded_word(tail: &[u8]) -> Option<(usize, String)> {
    // RFC 2047 words have a 75-byte maximum. Bounded lookahead prevents
    // repeated malformed prefixes from rescanning an entire long filename.
    let tail = &tail[..tail.len().min(75)];
    if !tail.starts_with(b"=?") {
        return None;
    }
    let charset_end = tail[2..].iter().position(|byte| *byte == b'?')? + 2;
    let encoding = *tail.get(charset_end + 1)?;
    if tail.get(charset_end + 2) != Some(&b'?') {
        return None;
    }
    let content = charset_end + 3;
    let end = tail[content..].windows(2).position(|pair| pair == b"?=")? + content;
    let payload = &tail[content..end];
    if payload.is_empty()
        || payload
            .iter()
            .any(|byte| !(33..=126).contains(byte) || *byte == b'?')
    {
        return None;
    }
    let bytes = match encoding {
        b'B' | b'b' => base64::engine::general_purpose::STANDARD
            .decode(payload)
            .ok()?,
        b'Q' | b'q' => {
            let mut result = Vec::new();
            let mut cursor = 0;
            while cursor < payload.len() {
                match payload[cursor] {
                    b'_' => result.push(b' '),
                    b'=' => {
                        let hi = (*payload.get(cursor + 1)? as char).to_digit(16)?;
                        let lo = (*payload.get(cursor + 2)? as char).to_digit(16)?;
                        result.push((hi * 16 + lo) as u8);
                        cursor += 2;
                    }
                    byte => result.push(byte),
                }
                cursor += 1;
            }
            result
        }
        _ => return None,
    };
    let charset = encoding_rs::Encoding::for_label(&tail[2..charset_end])?;
    let (text, _, errors) = charset.decode(&bytes);
    if errors {
        None
    } else {
        Some((end + 2, text.into_owned()))
    }
}
fn decode_words(value: &[u8]) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let mut previous_word = false;
    while cursor < value.len() {
        if previous_word {
            let whitespace = value[cursor..]
                .iter()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
            if whitespace > 0
                && let Some((length, text)) = encoded_word(&value[cursor + whitespace..])
            {
                output.push_str(&text);
                cursor += whitespace + length;
                continue;
            }
        }
        let tail = &value[cursor..];
        if let Some((length, text)) = encoded_word(tail) {
            output.push_str(&text);
            cursor += length;
            previous_word = true;
        } else {
            let next = tail
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, byte)| **byte == b'=')
                .map_or(tail.len(), |(index, _)| index);
            output.push_str(&String::from_utf8_lossy(&tail[..next]));
            cursor += next;
            previous_word = false;
        }
    }
    output
}
pub(crate) fn display_name(
    disposition: Option<&Structured>,
    content_type: &Structured,
) -> Result<Option<String>, PreparationError> {
    let selected = if let Some(cd) = disposition {
        parameter_value(&cd.parameters, b"filename", false)?
    } else {
        None
    };
    let selected = match selected {
        Some(value) => Some(value),
        None => parameter_value(&content_type.parameters, b"name", false)?,
    };
    let Some(DecodedParameter { bytes, charset }) = selected else {
        return Ok(None);
    };
    let bytes = if let Some(encoding) = charset
        .as_deref()
        .and_then(encoding_rs::Encoding::for_label)
    {
        encoding.decode(&bytes).0.into_owned().into_bytes()
    } else {
        bytes
    };
    let text: String = decode_words(&bytes)
        .chars()
        .filter(|c| !matches!(*c as u32, 0..=31 | 127..=159))
        .take(255)
        .collect();
    Ok((!text.is_empty()).then_some(text))
}
