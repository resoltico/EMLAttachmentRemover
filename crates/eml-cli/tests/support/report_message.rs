//! Synthetic independently constructed message and retained-byte oracle.
pub(crate) fn large_report_message() -> (Vec<u8>, Vec<u8>) {
    let prefix=b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=owned\r\n\r\n--owned\r\nContent-Type: text/plain\r\n\r\nbody\r\n--owned\r\nContent-Type: text/html\r\n\r\n<p>body</p>\r\n";
    let closing = b"--owned--\r\n";
    let mut input = prefix.to_vec();
    let name = "x".repeat(200);
    for _ in 0..5000 {
        input.extend_from_slice(format!("--owned\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{name}\"\r\n\r\nx\r\n").as_bytes());
    }
    input.extend_from_slice(closing);
    let mut golden = prefix.to_vec();
    golden.extend_from_slice(closing);
    (input, golden)
}
