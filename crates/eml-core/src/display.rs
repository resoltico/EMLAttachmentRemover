//! Human path and text presentation, independent of wire serialization.
use std::{
    fmt::{self, Write},
    path::Path,
};

/// Human display of a native path, preserving printable Unicode and exposing
/// invalid native units: Unix bytes as `<0xFF>`, unpaired UTF-16 as `<U+D800>`.
/// Literal `<` is doubled, distinguishing native-unit markers from filename text.
/// Controls use the same escapes as [`DisplayText`]. Native-unit distinctions are
/// preserved; this is not a general reversible encoding of control characters,
/// shell quoting, or an operational path value. Protocol-1 serialization is separate.
pub struct DisplayPath<'a>(&'a Path);
impl<'a> DisplayPath<'a> {
    /// Borrow the native path without resolving or changing its spelling.
    pub fn new(path: &'a Path) -> Self {
        Self(path)
    }
}
impl fmt::Display for DisplayPath<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            for chunk in self.0.as_os_str().as_bytes().utf8_chunks() {
                for character in chunk.valid().chars() {
                    path_character(character, formatter)?;
                }
                for byte in chunk.invalid() {
                    write!(formatter, "<0x{byte:02X}>")?;
                }
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            for character in char::decode_utf16(self.0.as_os_str().encode_wide()) {
                match character {
                    Ok(character) => path_character(character, formatter)?,
                    Err(error) => write!(formatter, "<U+{:04X}>", error.unpaired_surrogate())?,
                }
            }
        }
        Ok(())
    }
}
fn path_character(character: char, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    if character == '<' {
        formatter.write_str("<<")
    } else {
        display_character(character, formatter)
    }
}

/// Human text with control, line-separator and bidi-formatting characters made
/// explicit. Printable international text is preserved. Never an operational value.
pub struct DisplayText<'a>(&'a str);
impl<'a> DisplayText<'a> {
    /// Borrow untrusted text for human presentation.
    pub fn new(text: &'a str) -> Self {
        Self(text)
    }
}
impl fmt::Display for DisplayText<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            display_character(character, formatter)?;
        }
        Ok(())
    }
}
fn display_character(character: char, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    if character.is_control()
        || matches!(character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    {
        for escaped in character.escape_default() {
            formatter.write_char(escaped)?;
        }
        Ok(())
    } else {
        formatter.write_char(character)
    }
}
