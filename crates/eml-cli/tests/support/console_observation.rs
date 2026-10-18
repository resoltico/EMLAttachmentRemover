//! Native console-control sender only; excluded from every product archive.
#![forbid(unsafe_code)]
fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    if let Some(code) = eml_windows::console_helper_entry() {
        return code;
    }
    std::process::ExitCode::FAILURE
}
