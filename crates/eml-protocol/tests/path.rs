//! Output transport oracles contain independently reviewed native bytes, not file I/O.
#![cfg(any(unix, windows))]
use eml_protocol::{DisplayPath, DisplayText, PathValue};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[test]
fn ascii_path_matches_the_hand_authored_wire_value() {
    let path = Path::new("mail/a.eml");
    let actual = serde_json::to_string(&PathValue::new(path, path)).unwrap();
    #[cfg(unix)]
    let expected = r#"{"display":"mail/a.eml","native":{"encoding":"unix-bytes","base64":"bWFpbC9hLmVtbA=="}}"#;
    #[cfg(windows)]
    let expected = r#"{"display":"mail/a.eml","native":{"encoding":"windows-utf16le","base64":"bQBhAGkAbAAvAGEALgBlAG0AbAA="}}"#;
    assert_eq!(actual, expected);
}

#[test]
fn nonunicode_native_units_remain_distinct_from_lossy_display() {
    #[cfg(unix)]
    let (path, expected) = {
        use std::os::unix::ffi::OsStringExt;
        (
            PathBuf::from(OsString::from_vec(vec![0x61, 0xff, 0x62])),
            r#"{"display":"a�b","native":{"encoding":"unix-bytes","base64":"Yf9i"}}"#,
        )
    };
    #[cfg(windows)]
    let (path, expected) = {
        use std::os::windows::ffi::OsStringExt;
        (
            PathBuf::from(OsString::from_wide(&[0x61, 0xd800, 0x62])),
            r#"{"display":"a�b","native":{"encoding":"windows-utf16le","base64":"YQAA2GIA"}}"#,
        )
    };
    assert_eq!(
        serde_json::to_string(&PathValue::new(&path, &path)).unwrap(),
        expected
    );
}

#[test]
fn output_preserves_spelling_and_json_strings_do_not_introduce_physical_lf() {
    for spelling in [
        "alias/.././mail.eml",
        "mail/\n\u{85}\u{2028}\u{2029}\u{1b}\\\".eml",
    ] {
        let path = Path::new(spelling);
        let encoded = serde_json::to_string(&PathValue::new(path, path)).unwrap();
        assert!(!encoded.as_bytes().contains(&b'\n'));
        let object: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(object["display"], spelling);
        if spelling.contains('\u{2028}') {
            assert!(encoded.contains('\u{85}'));
            assert!(encoded.contains('\u{2028}'));
            assert!(encoded.contains('\u{2029}'));
            assert!(encoded.contains(r"\n"));
            assert!(encoded.contains(r"\u001b"));
        }
    }
}

#[test]
fn distinct_core_display_and_native_facts_match_hand_authored_wire_bytes() {
    let actual =
        serde_json::to_string(&PathValue::new(Path::new("A"), Path::new("request alias"))).unwrap();
    #[cfg(unix)]
    let expected =
        r#"{"display":"request alias","native":{"encoding":"unix-bytes","base64":"QQ=="}}"#;
    #[cfg(windows)]
    let expected =
        r#"{"display":"request alias","native":{"encoding":"windows-utf16le","base64":"QQA="}}"#;
    assert_eq!(actual, expected);
}

#[test]
fn display_path_matches_hand_authored_control_and_printable_oracles() {
    let path = Path::new("mail/\n\r\t\u{1b}[31m\u{85}\u{2028}\u{2029}\u{202e}\u{2066}.eml");
    assert_eq!(
        DisplayPath::new(path).to_string(),
        r"mail/\n\r\t\u{1b}[31m\u{85}\u{2028}\u{2029}\u{202e}\u{2066}.eml"
    );
    let printable = "mail/日本語/👩\u{200d}💻/a\\b-'\".eml";
    assert_eq!(
        DisplayPath::new(Path::new(printable)).to_string(),
        printable
    );
}

#[test]
fn human_text_preserves_readable_scripts_and_exposes_formatting_controls() {
    let input = "日本語-مرحبا-\u{202e}report\n\u{2066}file\u{2069}.eml";
    let expected = r"日本語-مرحبا-\u{202e}report\n\u{2066}file\u{2069}.eml";
    assert_eq!(DisplayText::new(input).to_string(), expected);
    assert_eq!(DisplayPath::new(Path::new(input)).to_string(), expected);
    assert_eq!(
        DisplayText::new("日本語 مرحبا 👩‍💻").to_string(),
        "日本語 مرحبا 👩‍💻"
    );
}

#[test]
fn display_path_leaves_no_c0_c1_line_or_bidi_controls_in_display() {
    for unit in (0..=0x1f)
        .chain(0x7f..=0x9f)
        .chain([0x061c, 0x200e, 0x200f])
        .chain(0x2028..=0x202e)
        .chain(0x2066..=0x2069)
    {
        let character = char::from_u32(unit).unwrap();
        let input = format!("before{character}after");
        let displayed = DisplayPath::new(Path::new(&input)).to_string();
        assert!(!displayed.contains(character), "unescaped U+{unit:04X}");
        let escaped = match unit {
            9 => String::from(r"\t"),
            10 => String::from(r"\n"),
            13 => String::from(r"\r"),
            _ => format!("\\u{{{unit:x}}}"),
        };
        assert_eq!(displayed, format!("before{escaped}after"));
    }
}

#[test]
#[cfg(unix)]
fn display_path_preserves_invalid_bytes_without_impersonating_filename_text() {
    use std::os::unix::ffi::OsStrExt;
    let cases: &[(&[u8], &str)] = &[
        (b"odd-\xff.eml", "odd-<0xFF>.eml"),
        (b"odd-\xfe.eml", "odd-<0xFE>.eml"),
        ("odd-�.eml".as_bytes(), "odd-�.eml"),
        (b"odd-<0xFF>.eml", "odd-<<0xFF>.eml"),
        (b"\xe6\x97\xa5\xff\xc3\xa9\xe2\x82", "日<0xFF>é<0xE2><0x82>"),
        (b"\x80\xf0\x9f", "<0x80><0xF0><0x9F>"),
        (b"valid?/#/a\\b", r"valid?/#/a\b"),
    ];
    for (bytes, expected) in cases {
        let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
        assert_eq!(DisplayPath::new(path).to_string(), *expected, "{bytes:?}");
    }
}

#[test]
#[cfg(windows)]
fn display_path_preserves_surrogate_pairs_and_distinguishes_unpaired_units() {
    use std::os::windows::ffi::OsStringExt;
    for (units, expected) in [
        (vec![0xD800], "<U+D800>"),
        (vec![0xD801], "<U+D801>"),
        (vec![0xFFFD], "�"),
        ("<U+D800>".encode_utf16().collect(), "<<U+D800>"),
        (
            vec![0x43, 0x3A, 0x5C, 0xD83D, 0xDC69, 0x200D, 0xD83D, 0xDCBB],
            "C:\\👩‍💻",
        ),
    ] {
        let path = PathBuf::from(OsString::from_wide(&units));
        assert_eq!(DisplayPath::new(&path).to_string(), expected);
    }
}

#[test]
fn native_unit_markers_do_not_double_escape_controls_or_change_text_formatting() {
    assert_eq!(
        DisplayPath::new(Path::new("<literal>\n\u{202e}\\n")).to_string(),
        r"<<literal>\n\u{202e}\n"
    );
    assert_eq!(DisplayText::new("<literal>\n").to_string(), r"<literal>\n");
}
