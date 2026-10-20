//! Static owned alias setup through the ordinary native provider.
use std::path::Path;

pub(crate) fn create(target: &Path, alias: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, alias).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        // Give PowerShell's junction provider ordinary local-drive arguments;
        // a canonical build cwd can supply verbatim spellings instead.
        // Resolve only owned fixture paths and retain their exact UTF-16 units.
        let provider_path = |path: &Path| {
            let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
            assert!(
                units.len() >= 7
                    && units[..4] == [92, 92, 63, 92]
                    && (units[4] as u8).is_ascii_alphabetic()
                    && units[4] <= 127
                    && units[5..7] == [58, 92],
                "Owned junction fixture needs a canonical local-drive path: {path:?}"
            );
            assert!(
                units.len() < 248,
                "Owned junction provider path exceeds its supported fixture bound"
            );
            std::ffi::OsString::from_wide(&units[4..])
        };
        let actual_target = target.canonicalize().unwrap();
        let actual_alias = alias
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join(alias.file_name().unwrap());
        // No profile, path interpolation, privilege adjustment or post-bind mutation.
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:EML_OWNED_ALIAS -Target $env:EML_OWNED_TARGET | Out-Null"])
            .env("EML_OWNED_ALIAS", provider_path(&actual_alias)).env("EML_OWNED_TARGET", provider_path(&actual_target)).status().unwrap();
        assert!(status.success());
        let resolved = std::fs::canonicalize(alias)
            .unwrap_or_else(|error| panic!("Owned junction does not resolve: {alias:?}: {error}"));
        assert_eq!(
            resolved, actual_target,
            "Owned junction resolved to another directory"
        );
    }
}
