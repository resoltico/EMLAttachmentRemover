//! Read the invoking workspace's tool pins, not an executable's build-time copy.
use crate::Result;
use std::collections::BTreeMap;
pub(crate) fn read() -> Result<BTreeMap<String, String>> {
    let value: toml::Value = toml::from_str(&std::fs::read_to_string("xtask/tools.toml")?)?;
    let table = value
        .get("versions")
        .and_then(toml::Value::as_table)
        .ok_or("missing tool versions table")?;
    table
        .iter()
        .map(|(key, value)| {
            let version = value.as_str().ok_or("tool pin must be a string")?;
            if key.is_empty()
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || version.is_empty()
                || !version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
            {
                return Err("tool pins must be nonempty single-line versions".into());
            }
            Ok((key.clone(), version.to_owned()))
        })
        .collect()
}
pub(crate) fn print() -> Result<()> {
    for (key, value) in read()? {
        println!("{key}={value}");
    }
    Ok(())
}
/// Check the same SDK pin in local and hosted native Linux gates.
pub(crate) fn qt() -> Result<()> {
    let versions = read()?;
    let expected = versions.get("qt").ok_or("missing Qt SDK pin")?;
    let qmake = std::env::var_os("QMAKE").unwrap_or_else(|| "qmake".into());
    let reported =
        crate::capture(std::process::Command::new(qmake).args(["-query", "QT_VERSION"]))?;
    if &reported != expected {
        return Err(format!("Qt SDK must be {expected}; qmake reported {reported}").into());
    }
    Ok(())
}
pub(crate) fn reported_version<'a>(name: &str, output: &'a str) -> Option<&'a str> {
    match name {
        "cargo-deny" | "cargo-about" => output
            .lines()
            .next()?
            .strip_prefix(name)?
            .strip_prefix(' ')?
            .split_whitespace()
            .next(),
        "actionlint" => output
            .lines()
            .next()?
            .split_whitespace()
            .next()
            .map(|v| v.trim_start_matches('v')),
        "shellcheck" => output
            .lines()
            .find_map(|line| line.strip_prefix("version: ")),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn version_checks_do_not_accept_incidental_numbers() {
        assert_eq!(
            super::reported_version("cargo-deny", "cargo-deny 0.20.1 built with 0.20.2"),
            Some("0.20.1")
        );
        assert_eq!(
            super::reported_version("cargo-about", "not-cargo-about 0.9.3"),
            None
        );
        assert_eq!(
            super::reported_version("shellcheck", "ShellCheck\nversion: 0.11.0\n"),
            Some("0.11.0")
        );
    }
}
