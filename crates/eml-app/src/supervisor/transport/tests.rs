use super::*;
fn wait_reader(role: &mut ReadRole) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let error = role.poll();
        if role.done() {
            return error;
        }
        assert!(Instant::now() < deadline, "Owned reader did not finish");
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn panicking_reader_retains_endpoint_until_join_and_checked_close() {
    let (reader, writer) = io::pipe().unwrap();
    let mut role = ReadRole::new(Some(reader));
    role.start("owned-reader-panic", |_| panic!("synthetic reader failure"));
    assert_eq!(wait_reader(&mut role).as_deref(), Some("Reader panicked"));
    let result = role.take();
    assert!(result.closed);
    assert!(!result.eof);
    assert!(
        role.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    );
    close_writer(writer).unwrap();
}
#[test]
fn ordinary_reader_requires_actual_eof_and_checked_close() {
    let (reader, mut writer) = io::pipe().unwrap();
    let mut role = ReadRole::new(Some(reader));
    role.start("owned-reader-eof", |reader| capture(reader, 3));
    writer.write_all(b"five!").unwrap();
    close_writer(writer).unwrap();
    assert!(wait_reader(&mut role).is_none());
    let result = role.take();
    assert!(result.closed && result.eof && result.truncated);
    assert_eq!(result.total_bytes, 5);
    assert_eq!(result.bytes, b"fiv");
}
