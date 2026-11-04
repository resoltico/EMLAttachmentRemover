//! Source identity validation shared by compilation and distribution.
pub(crate) fn revision(value: &str) -> Result<(), &'static str> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("EML_SOURCE_REVISION must be a full 40- or 64-character ASCII hexadecimal commit ID")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_revisions_require_complete_ascii_hexadecimal_ids() {
        for value in ["a".repeat(40), "A0".repeat(32)] {
            assert!(super::revision(&value).is_ok());
        }
        for value in [
            String::new(),
            "a".repeat(12),
            "a".repeat(41),
            "z".repeat(40),
            "é".repeat(20),
            format!("{}\n", "a".repeat(40)),
        ] {
            assert!(super::revision(&value).is_err(), "{value}");
        }
    }
}
