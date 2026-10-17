//! Nonshipping owned child for actual signal-mask, stdio and lifetime checks.
#[cfg(not(target_os = "macos"))]
compile_error!("Native macOS child fixtures require a macOS host");
#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "Read only this fixture's own process signal mask"
)]
fn main() {
    use std::io::{Read, Write};
    // SAFETY: query writes a valid sigset_t; no parent/global signal state changes.
    let blocked = unsafe {
        let mut set = std::mem::zeroed::<libc::sigset_t>();
        assert_eq!(libc::sigprocmask(0, std::ptr::null(), &mut set), 0);
        libc::sigismember(&set, libc::SIGINT)
    };
    println!("int-blocked={blocked}");
    std::io::stdout().flush().unwrap();
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(65537)
        .read_to_end(&mut input)
        .unwrap();
    assert!(input.len() <= 65536);
    std::io::stdout().write_all(&input).unwrap();
    eprintln!("owned fixture reached EOF");
    std::process::exit(7);
}
