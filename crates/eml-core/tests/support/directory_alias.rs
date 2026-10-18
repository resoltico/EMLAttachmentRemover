//! Static owned alias setup through the ordinary native provider.
use std::path::Path;

pub fn create(target: &Path, alias: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, alias).unwrap();
    #[cfg(windows)]
    {
        // No profile, path interpolation, privilege adjustment or post-bind mutation.
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:EML_OWNED_ALIAS -Target $env:EML_OWNED_TARGET | Out-Null"])
            .env("EML_OWNED_ALIAS", alias).env("EML_OWNED_TARGET", target).status().unwrap();
        assert!(status.success());
        let resolved = std::fs::canonicalize(alias)
            .unwrap_or_else(|error| panic!("Owned junction does not resolve: {alias:?}: {error}"));
        assert_eq!(
            resolved,
            target.canonicalize().unwrap(),
            "Owned junction resolved to another directory"
        );
    }
}
