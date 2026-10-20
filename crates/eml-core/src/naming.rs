//! Native copy components; no lossy conversion, collision probing or filesystem writes.
use std::ffi::{OsStr, OsString};

/// Fixed copy-name suffix, also recognized when skipping directory-walk outputs.
pub const COPY_SUFFIX: &str = ".no-attachments.eml";

/// A source component or destination limit cannot supply the required copy name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamingError {
    /// The input is not one structural native filename component.
    InvalidComponent,
    /// Even the fixed copy suffix exceeds the destination's component limit.
    SuffixDoesNotFit,
}
impl std::fmt::Display for NamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidComponent => "Copy naming requires one native filename component",
            Self::SuffixDoesNotFit => {
                "The destination component limit cannot fit the fixed copy suffix"
            }
        })
    }
}
impl std::error::Error for NamingError {}

/// Construct the copy basename under an actual native-unit limit. Only the stem
/// is truncated; Unicode boundaries and valid UTF-16 pairs are preserved.
/// This is structural naming, not filesystem admission. The resulting component
/// must pass the native adapter's validation, including Windows reserved names
/// that truncation can create, before any open or creation.
pub fn copy_basename(source: &OsStr, max_units: usize) -> Result<OsString, NamingError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let source = source.as_bytes();
        if source.is_empty()
            || source == b"."
            || source == b".."
            || source.contains(&0)
            || source.contains(&b'/')
        {
            return Err(NamingError::InvalidComponent);
        }
        let capacity = max_units
            .checked_sub(COPY_SUFFIX.len())
            .ok_or(NamingError::SuffixDoesNotFit)?;
        let stem = if source.len() >= 4 && source[source.len() - 4..].eq_ignore_ascii_case(b".eml")
        {
            &source[..source.len() - 4]
        } else {
            source
        };
        let stem = if stem.is_empty() { b"message" } else { stem };
        let mut length = stem.len().min(capacity);
        if let Ok(unicode) = std::str::from_utf8(stem) {
            while !unicode.is_char_boundary(length) {
                length -= 1;
            }
        }
        let mut name = stem[..length].to_vec();
        name.extend_from_slice(COPY_SUFFIX.as_bytes());
        Ok(OsString::from_vec(name))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        Ok(OsString::from_wide(&copy_utf16(
            &source.encode_wide().collect::<Vec<_>>(),
            max_units,
        )?))
    }
}

#[cfg(any(windows, test))]
fn copy_utf16(source: &[u16], max_units: usize) -> Result<Vec<u16>, NamingError> {
    if source.is_empty()
        || source == [46]
        || source == [46, 46]
        || source.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
    {
        return Err(NamingError::InvalidComponent);
    }
    let suffix: Vec<_> = COPY_SUFFIX.encode_utf16().collect();
    let capacity = max_units
        .checked_sub(suffix.len())
        .ok_or(NamingError::SuffixDoesNotFit)?;
    let extension = *b".eml";
    let ends_eml = source.len() >= 4
        && source[source.len() - 4..]
            .iter()
            .zip(extension)
            .all(|(unit, expected)| {
                *unit == u16::from(expected) || *unit == u16::from(expected.to_ascii_uppercase())
            });
    let stem = if ends_eml {
        &source[..source.len() - 4]
    } else {
        source
    };
    let fallback: Vec<_> = "message".encode_utf16().collect();
    let stem = if stem.is_empty() { &fallback } else { stem };
    let mut length = stem.len().min(capacity);
    if length != 0
        && length < stem.len()
        && (0xd800..=0xdbff).contains(&stem[length - 1])
        && (0xdc00..=0xdfff).contains(&stem[length])
    {
        length -= 1;
    }
    let mut name = stem[..length].to_vec();
    name.extend(suffix);
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_use_the_final_extension_fallback_and_fixed_suffix_only() {
        for (source, expected) in [
            ("letter.EML", "letter.no-attachments.eml"),
            ("letter.eml.backup", "letter.eml.backup.no-attachments.eml"),
            (".eml", "message.no-attachments.eml"),
            ("plain", "plain.no-attachments.eml"),
        ] {
            assert_eq!(copy_basename(OsStr::new(source), 255).unwrap(), expected);
        }
        assert_eq!(
            copy_basename(OsStr::new("letter.eml"), 18),
            Err(NamingError::SuffixDoesNotFit)
        );
        assert_eq!(
            copy_basename(OsStr::new("letter.eml"), 19).unwrap(),
            ".no-attachments.eml"
        );
        // These basenames are valid under another parent. Beside-source identity
        // reservations, rather than a basename-only ban, must reject self-output.
        assert_eq!(
            copy_basename(OsStr::new(".no-attachments.eml"), 19).unwrap(),
            ".no-attachments.eml"
        );
        assert_eq!(
            copy_basename(OsStr::new("long.no-attachments.eml"), 23).unwrap(),
            "long.no-attachments.eml"
        );
        for source in ["", ".", "..", "a/b", "a\0b"] {
            assert_eq!(
                copy_basename(OsStr::new(source), 255),
                Err(NamingError::InvalidComponent)
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn unix_truncation_preserves_unicode_or_exact_nonunicode_units() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        assert_eq!(
            copy_basename(OsStr::new("a🦀b.eml"), 23).unwrap(),
            "a.no-attachments.eml"
        );
        assert_eq!(
            copy_basename(OsStr::new("a🦀b.eml"), 24).unwrap(),
            "a🦀.no-attachments.eml"
        );
        let source = OsString::from_vec(b"a\xffb.EML".to_vec());
        assert_eq!(
            copy_basename(&source, 21).unwrap().as_bytes(),
            b"a\xff.no-attachments.eml"
        );
        let long = "z".repeat(1000);
        let name = copy_basename(OsStr::new(&long), 255).unwrap();
        assert_eq!(name.as_bytes().len(), 255);
        assert!(name.as_bytes().ends_with(b".no-attachments.eml"));
    }
    #[test]
    fn utf16_preserves_paired_boundaries_and_unpaired_surrogates() {
        let source: Vec<_> = "a🦀b.EML".encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&copy_utf16(&source, 21).unwrap()).unwrap(),
            "a.no-attachments.eml"
        );
        assert_eq!(
            String::from_utf16(&copy_utf16(&source, 22).unwrap()).unwrap(),
            "a🦀.no-attachments.eml"
        );
        let actual = copy_utf16(&[0xd800, 120, 46, 101, 109, 108], 20).unwrap();
        assert_eq!(actual[0], 0xd800);
        assert_eq!(
            &actual[1..],
            ".no-attachments.eml".encode_utf16().collect::<Vec<_>>()
        );
        let mixed = copy_utf16(&[0xd800, 0xd801, 0xdc00, 46, 101, 109, 108], 21).unwrap();
        assert_eq!(mixed[0], 0xd800);
        assert_eq!(
            &mixed[1..],
            ".no-attachments.eml".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            copy_utf16(&[46, 101, 109, 108], 255).unwrap(),
            "message.no-attachments.eml"
                .encode_utf16()
                .collect::<Vec<_>>()
        );
        for source in [&[0][..], &[58], &[92], &[47], &[46, 46]] {
            assert_eq!(copy_utf16(source, 255), Err(NamingError::InvalidComponent));
        }
    }
    #[cfg(windows)]
    #[test]
    fn native_windows_component_round_trips_unpaired_units_and_counts_pairs() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let source = OsString::from_wide(&[0xd800, 120, 46, 69, 77, 76]);
        let actual: Vec<_> = copy_basename(&source, 20).unwrap().encode_wide().collect();
        assert_eq!(actual[0], 0xd800);
        assert_eq!(
            &actual[1..],
            ".no-attachments.eml".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            copy_basename(OsStr::new("a🦀b.eml"), 22).unwrap(),
            "a🦀.no-attachments.eml"
        );
    }
}
