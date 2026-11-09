//! Native desktop entry point; this frontend renders core directly.
#![deny(unsafe_code)]
fn main() {
    if let Err(error) = eml_gui::run_desktop(eml_core::run) {
        eprintln!("{}: {error}", eml_core::product::DISPLAY_NAME);
        std::process::exit(1);
    }
}
