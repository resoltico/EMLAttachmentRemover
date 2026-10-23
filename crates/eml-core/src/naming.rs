//! Exact native destination-route encoding and generated component grammar.
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

/// Fixed copy-name suffix, also recognized when skipping directory-walk outputs.
pub const COPY_SUFFIX: &str = ".no-attachments.eml";
pub(crate) const MIRROR_SUFFIX: &str = ".eml-dir";
const DOMAIN: &[u8] = b"eml-attachment-remover:destination-route:v1\0";

/// A source component or destination limit cannot supply the required name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamingError {
    /// The input is not one structural native filename component.
    InvalidComponent,
    /// The fixed grammar and at least one whole human character cannot fit.
    SuffixDoesNotFit,
}
impl std::fmt::Display for NamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidComponent => "Destination naming requires one native filename component",
            Self::SuffixDoesNotFit => "Destination component limit cannot fit the fixed grammar and a whole human character",
        })
    }
}
impl std::error::Error for NamingError {}

/// Truncated SHA-256 of the complete framed raw logical route; never file identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RouteToken([u8; 16]);
impl RouteToken {
    fn text(self) -> String {
        const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
        let mut out = String::with_capacity(26);
        let mut bits = 0u32;
        let mut count = 0;
        for byte in self.0 {
            bits = (bits << 8) | u32::from(byte);
            count += 8;
            while count >= 5 {
                count -= 5;
                out.push(ALPHABET[((bits >> count) & 31) as usize] as char);
            }
        }
        if count > 0 {
            out.push(ALPHABET[((bits << (5 - count)) & 31) as usize] as char);
        }
        out
    }
}

fn valid_component(source: &OsStr) -> bool {
    let path = Path::new(source);
    let mut parts = path.components();
    matches!(parts.next(), Some(std::path::Component::Normal(_)))
        && parts.next().is_none()
        && !source.as_encoded_bytes().contains(&0)
}
fn stem(source: &OsStr) -> Result<OsString, NamingError> {
    if !valid_component(source) {
        return Err(NamingError::InvalidComponent);
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = source.as_bytes();
        let bytes = if bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".eml") {
            &bytes[..bytes.len() - 4]
        } else {
            bytes
        };
        Ok(OsString::from_vec(if bytes.is_empty() {
            b"message".to_vec()
        } else {
            bytes.to_vec()
        }))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let mut units: Vec<_> = source.encode_wide().collect();
        if units.iter().any(|u| matches!(*u, 0 | 47 | 58 | 92)) {
            return Err(NamingError::InvalidComponent);
        }
        if units.len() >= 4
            && units[units.len() - 4..]
                .iter()
                .zip(b".eml")
                .all(|(u, b)| *u == u16::from(*b) || *u == u16::from(b.to_ascii_uppercase()))
        {
            units.truncate(units.len() - 4);
        }
        if units.is_empty() {
            units.extend("message".encode_utf16());
        }
        Ok(OsString::from_wide(&units))
    }
}
pub(crate) fn logical_basename(source: &OsStr) -> Result<OsString, NamingError> {
    let mut name = stem(source)?;
    name.push(COPY_SUFFIX);
    Ok(name)
}
fn hash_component(hash: &mut Sha256, kind: u8, name: &OsStr) {
    if let Some(text) = name.to_str() {
        hash.update([kind, 0]);
        hash.update((text.len() as u32).to_le_bytes());
        hash.update(text.as_bytes());
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let bytes = name.as_bytes();
            hash.update([kind, 1]);
            hash.update((bytes.len() as u32).to_le_bytes());
            hash.update(bytes);
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            hash.update([kind, 2]);
            hash.update(((name.encode_wide().count() * 2) as u32).to_le_bytes());
            for unit in name.encode_wide() {
                hash.update(unit.to_le_bytes());
            }
        }
    }
}
pub(crate) fn route_token(route: &Path, source: &OsStr) -> Result<RouteToken, NamingError> {
    let file = logical_basename(source)?;
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(((route.components().count() + 1) as u32).to_le_bytes());
    for part in route.components() {
        hash_component(&mut hash, 1, part.as_os_str());
    }
    hash_component(&mut hash, 2, &file);
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Ok(RouteToken(bytes))
}
fn render(human: &OsStr, suffix: &str, max_units: usize) -> Result<OsString, NamingError> {
    let capacity = max_units
        .checked_sub(suffix.len())
        .ok_or(NamingError::SuffixDoesNotFit)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = human.as_bytes();
        let mut length = bytes.len().min(capacity);
        if let Ok(text) = std::str::from_utf8(bytes) {
            while !text.is_char_boundary(length) {
                length -= 1;
            }
        }
        if length == 0 {
            return Err(NamingError::SuffixDoesNotFit);
        }
        let mut name = bytes[..length].to_vec();
        name.extend(suffix.as_bytes());
        Ok(OsString::from_vec(name))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        Ok(OsString::from_wide(&render_utf16(
            &human.encode_wide().collect::<Vec<_>>(),
            suffix,
            capacity,
        )?))
    }
}
#[cfg(any(windows, test))]
fn render_utf16(human: &[u16], suffix: &str, capacity: usize) -> Result<Vec<u16>, NamingError> {
    let mut length = human.len().min(capacity);
    if length > 0
        && length < human.len()
        && (0xd800..=0xdbff).contains(&human[length - 1])
        && (0xdc00..=0xdfff).contains(&human[length])
    {
        length -= 1;
    }
    if length == 0 {
        return Err(NamingError::SuffixDoesNotFit);
    }
    let mut name = human[..length].to_vec();
    name.extend(suffix.encode_utf16());
    Ok(name)
}
/// Construct an exact beside-source copy name. Only its human stem is shortened.
/// Planning a mirrored output uses the complete raw route instead.
pub fn copy_basename(source: &OsStr, max_units: usize) -> Result<OsString, NamingError> {
    route_copy_basename(source, route_token(Path::new(""), source)?, max_units)
}
pub(crate) fn route_copy_basename(
    source: &OsStr,
    token: RouteToken,
    max_units: usize,
) -> Result<OsString, NamingError> {
    render(
        &stem(source)?,
        &format!(".{}{COPY_SUFFIX}", token.text()),
        max_units,
    )
}
pub(crate) fn mirror_basename(source: &OsStr, max_units: usize) -> Result<OsString, NamingError> {
    if !valid_component(source) {
        return Err(NamingError::InvalidComponent);
    }
    render(source, MIRROR_SUFFIX, max_units)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_route_vectors() {
        for (route, source, expected) in [
            ("", "message.eml", "eudtfo5quf3d2hlohgknt7qpaa"),
            ("", "A.eml", "gsfuhldmedi75npfikxt6rlujm"),
            ("", "a.eml", "ulaixrqur2pkcd3knlgxwbxf6i"),
            ("Inbox/2026", "message.eml", "q5byg54wxmn4ov4dlnea4ogb7q"),
            ("", "résumé.eml", "i5axmvazzbp46ysadz3wci5byq"),
        ] {
            assert_eq!(
                route_token(Path::new(route), OsStr::new(source))
                    .unwrap()
                    .text(),
                expected
            );
        }
        assert_eq!(
            copy_basename(OsStr::new(".EML"), 255).unwrap(),
            "message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml"
        );
        assert_ne!(
            route_token(Path::new("A"), OsStr::new("x.eml")),
            route_token(Path::new("a"), OsStr::new("x.eml"))
        );
        assert_ne!(
            route_token(Path::new("ab/c"), OsStr::new("x.eml")),
            route_token(Path::new("a/bc"), OsStr::new("x.eml"))
        );
    }
    #[test]
    fn fixed_grammar_never_loses_human_or_token_units() {
        for limit in [0, 19, 45, 46] {
            assert_eq!(
                copy_basename(OsStr::new("a.eml"), limit),
                Err(NamingError::SuffixDoesNotFit)
            );
        }
        assert_eq!(
            copy_basename(OsStr::new("abcd.eml"), 47).unwrap(),
            "a.beg4w34u2itgc4omwdlcox3iw4.no-attachments.eml"
        );
        assert_eq!(
            mirror_basename(OsStr::new("x"), 8),
            Err(NamingError::SuffixDoesNotFit)
        );
        assert_eq!(mirror_basename(OsStr::new("xyz"), 9).unwrap(), "x.eml-dir");
        assert_eq!(
            render_utf16(&[0xd83e, 0xdd80, 120], ".eml-dir", 1),
            Err(NamingError::SuffixDoesNotFit)
        );
        assert_eq!(
            render_utf16(&[0xd800, 120], ".eml-dir", 1).unwrap()[0],
            0xd800
        );
        #[cfg(unix)]
        assert_eq!(
            mirror_basename(OsStr::new("🦀"), 9),
            Err(NamingError::SuffixDoesNotFit)
        );
    }
}

#[cfg(test)]
mod independent_native_vectors {
    use super::*;
    #[test]
    fn unicode_normalization_framing_pairs_and_maximal_components() {
        for (route, source, expected) in [
            ("", "é.eml", "mvvnv6ggb7vtnbb5eidfbxgq3m"),
            ("", "e\u{301}.eml", "w7nepcjblfyamzt2pfw3uib3xi"),
            ("ab", "c.eml", "edwhzbv46d2dpzc45sv45runzm"),
            ("a", "bc.eml", "vxq5qs5nfyifw4mcpfbsd62zzi"),
            ("", "crab-🦀.eml", "mmojonmtkxninjyqfo3z2wfop4"),
        ] {
            assert_eq!(
                route_token(Path::new(route), OsStr::new(source))
                    .unwrap()
                    .text(),
                expected
            );
        }
        let source = format!("{}.eml", "a".repeat(250));
        assert_eq!(
            route_token(Path::new(""), OsStr::new(&source))
                .unwrap()
                .text(),
            "igovl3kcph2bglpsukmukphgiu"
        );
        let rendered = copy_basename(OsStr::new(&source), 255).unwrap();
        assert_eq!(
            rendered,
            OsString::from(format!(
                "{}.igovl3kcph2bglpsukmukphgiu.no-attachments.eml",
                "a".repeat(209)
            ))
        );
        let mirror = "A".repeat(255);
        assert_eq!(
            route_token(Path::new(&mirror), OsStr::new("message.eml"))
                .unwrap()
                .text(),
            "iy3sl2f76cu6xnwggnpxrl27lu"
        );
        assert_eq!(
            mirror_basename(OsStr::new(&mirror), 255).unwrap(),
            OsString::from(format!("{}.eml-dir", "A".repeat(247)))
        );
    }
    #[cfg(unix)]
    #[test]
    fn invalid_unix_bytes_have_a_lossless_distinct_encoding() {
        use std::os::unix::ffi::OsStringExt;
        let source = OsString::from_vec(b"raw-\xff.eml".to_vec());
        assert_eq!(
            route_token(Path::new(""), &source).unwrap().text(),
            "yqzfd4hkueyil25rxwak4v7a7e"
        );
        assert_eq!(
            copy_basename(OsStr::new("é.eml"), 47),
            Err(NamingError::SuffixDoesNotFit)
        );
    }
    #[cfg(windows)]
    #[test]
    fn unpaired_windows_units_are_distinct_from_valid_utf8_and_pairs_stay_whole() {
        use std::os::windows::ffi::OsStringExt;
        let source = OsString::from_wide(&[114, 97, 119, 45, 0xd800, 46, 101, 109, 108]);
        assert_eq!(
            route_token(Path::new(""), &source).unwrap().text(),
            "vdpkcjjfvnirmkksilm2wftg3q"
        );
        assert_eq!(
            copy_basename(OsStr::new("🦀.eml"), 47),
            Err(NamingError::SuffixDoesNotFit)
        );
        assert!(copy_basename(OsStr::new("🦀.eml"), 48).is_ok());
    }
}
