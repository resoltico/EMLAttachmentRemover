//! Acceptance over the production controller and typed in-process core runs.
use eml_core::{Event, EventSink, InvalidRequest, RunControl, RunMode, RunOutcome, RunRequest};
use eml_gui::{DesktopController, RunSink, preferences::Preferences};
use i_slint_backend_testing::{init_no_event_loop, mock_elapsed_time};
use slint::ComponentHandle;
use std::{
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
const ATTACHMENT: &[u8] =
    include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const INLINE: &[u8] =
    include_bytes!("../../../tests/contract/mime/inputs/02-outlook-related-image.eml");
const PLAIN: &[u8] = b"Content-Type: text/plain\r\n\r\nbody\r\n";
fn write(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}
fn controller(
    root: &Path,
    runner: impl Fn(&RunRequest, &RunControl, &mut RunSink) -> Result<RunOutcome, InvalidRequest>
    + Send
    + Sync
    + 'static,
) -> Rc<DesktopController> {
    let c = DesktopController::new(
        runner,
        root.to_owned(),
        Preferences::load(root.join("preferences.json")),
    )
    .unwrap();
    c.window()
        .window()
        .set_size(slint::LogicalSize::new(1060., 760.));
    c.window().show().unwrap();
    mock_elapsed_time(Duration::from_millis(1));
    c
}
fn pump(c: &DesktopController, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        c.tick();
        mock_elapsed_time(Duration::from_millis(5));
        if predicate() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Core run did not reach expected state: {:?}, {}",
            c.window().get_view().state,
            c.window().get_view().banner
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn ready(root: &Path, names: &[&str]) -> Rc<DesktopController> {
    for name in names {
        write(root, name, ATTACHMENT);
    }
    let c = controller(root, eml_core::run);
    c.stage(vec![root.to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c
}
fn done(root: &Path, names: &[&str]) -> Rc<DesktopController> {
    let c = ready(root, names);
    c.start_selection();
    pump(&c, || !c.is_busy());
    c
}
struct Hook<'a, F> {
    target: &'a mut RunSink,
    emit: F,
}
impl<F> EventSink for Hook<'_, F>
where
    F: for<'a> FnMut(Event<'a>, &mut RunSink) -> io::Result<()>,
{
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        (self.emit)(event, self.target)
    }
}
fn run_with(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut RunSink,
    emit: impl for<'a> FnMut(Event<'a>, &mut RunSink) -> io::Result<()>,
) -> Result<RunOutcome, InvalidRequest> {
    eml_core::run(request, control, &mut Hook { target: sink, emit })
}
#[derive(Clone)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);
struct Release(Gate);
fn latch() -> (Release, Gate) {
    let gate = Gate(Arc::new((Mutex::new(false), Condvar::new())));
    (Release(gate.clone()), gate)
}
impl Gate {
    fn wait(&self) {
        let (lock, cv) = &*self.0;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = cv.wait(open).unwrap();
        }
    }
}
impl Release {
    fn release(&self) {
        let (lock, cv) = &*self.0.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}
#[path = "desktop/acceptance.rs"]
mod acceptance;
#[path = "desktop/report.rs"]
mod report;
#[path = "desktop/safety.rs"]
mod safety;
#[path = "desktop/source_guards.rs"]
mod source_guards;
