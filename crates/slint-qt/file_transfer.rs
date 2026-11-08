// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use std::path::PathBuf;

pub(crate) fn file_paths(urls: impl IntoIterator<Item = String>) -> Result<Vec<PathBuf>, String> {
    urls.into_iter()
        .map(|value| {
            let invalid = || "Data transfer contained an invalid local file URL".to_owned();
            let url = url::Url::parse(&value).map_err(|_| invalid())?;
            if url.query().is_some() || url.fragment().is_some() {
                return Err(invalid());
            }
            let path = url.to_file_path().map_err(|_| invalid())?;
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                if path.as_os_str().as_bytes().contains(&0) {
                    return Err(invalid());
                }
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                if path.as_os_str().encode_wide().any(|unit| unit == 0) {
                    return Err(invalid());
                }
            }
            Ok(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpp::cpp;
    cpp! {{ #include <QtCore/QUrl> }}

    #[test]
    fn rejects_remote_urls_without_a_partial_file_list() {
        assert!(
            file_paths([
                "file:///tmp/first.eml".into(),
                "https://example.org/a.eml".into()
            ])
            .is_err()
        );
        for invalid in [
            "not a URL",
            "file://remote/tmp/a.eml",
            "file:///tmp/a.eml?query",
            "file:///tmp/a.eml#fragment",
            "file:///tmp/%00.eml",
        ] {
            assert!(file_paths([invalid.into()]).is_err(), "{invalid}");
            assert!(file_paths(["file:///tmp/first.eml".into(), invalid.into()]).is_err());
        }
        let punctuation = file_paths(["file:///tmp/question%3Fhash%23.eml".into()]).unwrap();
        assert_eq!(punctuation[0], PathBuf::from("/tmp/question?hash#.eml"));
        assert!(file_paths(Vec::<String>::new()).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn encoded_qt_urls_preserve_native_path_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let encoded = qttypes::QString::from("file:///tmp/probe-%FF-%0A.eml");
        let lossy = cpp! {unsafe [encoded as "QString"] -> qttypes::QString as "QString" {
            return QUrl::fromEncoded(encoded.toLatin1()).toLocalFile();
        }};
        // Negative control: the original Qt backend converted through QString.
        assert_ne!(lossy.to_string().as_bytes(), b"/tmp/probe-\xff-\n.eml");
        let returned = cpp! {unsafe [encoded as "QString"] -> qttypes::QString as "QString" {
            auto url = QUrl::fromEncoded(encoded.toLatin1());
            return QString::fromLatin1(url.toEncoded(QUrl::FullyEncoded));
        }};
        let paths = file_paths([returned.to_string()]).unwrap();
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/probe-\xff-\n.eml");
    }
}
