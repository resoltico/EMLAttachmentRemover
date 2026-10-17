//! macOS Slint view over the reusable desktop application model.
#[allow(
    unsafe_code,
    reason = "Slint-generated vtables and embedding hooks only"
)]
mod view {
    slint::include_modules!();
}
use slint::ComponentHandle;
fn main() -> std::process::ExitCode {
    match start() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("EML Attachment Remover: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn start() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SLINT_SOFTWARE_RENDERER_PARLEY_DISABLED").is_some() {
        return Err("Unset SLINT_SOFTWARE_RENDERER_PARLEY_DISABLED: this app requires the desktop system-font shaping path".into());
    }
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;
    let window = view::MainWindow::new()?;
    let about = view::AboutWindow::new()?;
    about.set_identity(format!("EML Attachment Remover {}\nSoftware: MPL-2.0\nSlint: Royalty-free Desktop, Mobile, and Web Applications License2.0\nLocal build; native product qualification is incomplete.",env!("CARGO_PKG_VERSION")).into());
    let weak = about.as_weak();
    window.on_about_requested(move || {
        if let Some(about) = weak.upgrade()
            && let Err(error) = about.show()
        {
            eprintln!("About window: {error}");
        }
    });
    window.run()?;
    Ok(())
}
