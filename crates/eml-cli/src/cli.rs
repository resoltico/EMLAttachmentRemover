//! Process-local arguments, request transport, signals and checked channels only.
use crate::human::HumanSink;
use clap::{
    Parser, ValueEnum,
    error::{ContextKind, ContextValue, ErrorKind},
};
use eml_core::{InvalidRequest, OutputLocation, RunControl, RunMode, RunRequest};
use eml_protocol::{JsonSink, MAX_REQUEST_BYTES, TerminalPath, decode_request};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Human,
    Json,
}
#[derive(Debug, Parser)]
#[command(name=env!("CARGO_PKG_NAME"),bin_name=env!("CARGO_PKG_NAME"),version=eml_core::product::VERSION_WITH_BUILD.as_str(),about="Create verified body-only EML copies without modifying originals",color=clap::ColorChoice::Never,disable_help_subcommand=true,after_help="Examples:\n  emlattachmentremover -- message.eml\n  emlattachmentremover --dry-run --output-dir copies -- mail-folder\n  emlattachmentremover --format json -- message.eml")]
struct Arguments {
    /// Save copies in an existing folder; mirror selected directory trees.
    #[arg(long = "output-dir", value_name = "DIR", conflicts_with = "request")]
    output_directory: Option<PathBuf>,
    /// Check inputs and planned destinations without writing copies.
    #[arg(long, conflicts_with = "request")]
    dry_run: bool,
    /// Choose readable results or protocol-1 JSON lines.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
    /// Read a protocol-1 JSON request from FILE, or from stdin with '-'.
    #[arg(long,value_name="FILE|-",conflicts_with_all=["paths","output_directory","dry_run"])]
    request: Option<OsString>,
    /// EML files or folders to process. Use '--' before filenames starting with '-'.
    #[arg(value_name = "PATH", required_unless_present = "request")]
    paths: Vec<PathBuf>,
}
#[derive(Debug)]
enum InputError {
    Io(io::Error),
    TooLarge,
    Cancelled,
    Setup(io::Error),
    Disconnected,
}
fn bounded_read(mut reader: impl Read, control: &RunControl) -> Result<Vec<u8>, InputError> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        if control.is_stopped() {
            return Err(InputError::Cancelled);
        }
        let capacity = buffer.len().min(MAX_REQUEST_BYTES + 1 - bytes.len());
        let count = match reader.read(&mut buffer[..capacity]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(InputError::Io(error)),
        };
        if control.is_stopped() {
            return Err(InputError::Cancelled);
        }
        if count == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(InputError::TooLarge);
        }
    }
}
fn request_bytes(location: OsString, control: &RunControl) -> Result<Vec<u8>, InputError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker_control = control.clone();
    let handle = thread::Builder::new()
        .name("request-reader".into())
        .spawn(move || {
            // Open and stdin locking are here too: even an open/FIFO wait must not
            // block main's independent cancellation loop before any processing.
            let result = if location == OsStr::new("-") {
                let input = io::stdin();
                bounded_read(input.lock(), &worker_control)
            } else {
                match File::open(Path::new(&location)) {
                    Ok(file) => bounded_read(file, &worker_control),
                    Err(error) => Err(InputError::Io(error)),
                }
            };
            // Receiver disappearance is intentional on cancellation/process exit.
            // This transport worker never performs source/output business work.
            drop(sender.send(result));
        })
        .map_err(InputError::Setup)?;
    drop(handle); // Never join a request read which may be blocked in the OS.
    loop {
        if control.is_stopped() {
            return Err(InputError::Cancelled);
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(result) => {
                if control.is_stopped() {
                    return Err(InputError::Cancelled);
                }
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(InputError::Disconnected),
        }
    }
}
fn terminal(writer: &mut impl Write, text: &str) -> io::Result<()> {
    for line in text.split_inclusive('\n') {
        let newline = line.ends_with('\n');
        let value = if newline {
            &line[..line.len() - 1]
        } else {
            line
        };
        write!(writer, "{}", TerminalPath::new(Path::new(value)))?;
        if newline {
            writer.write_all(b"\n")?;
        }
    }
    writer.flush()
}
fn diagnostic(text: &str, code: u8, control: &RunControl) -> u8 {
    if control.is_stopped() {
        return 130;
    }
    let mut stderr = io::stderr().lock();
    match terminal(&mut stderr, &format!("{}\n", text.trim_end_matches('\n'))) {
        Ok(()) => {
            if control.is_stopped() {
                130
            } else {
                code
            }
        }
        Err(_) => {
            if control.is_stopped() {
                130
            } else {
                1
            }
        }
    }
}
// Escape untrusted parser context before Clap adds its own readable line breaks.
// Usage comes only from the fixed command definition, not from argv values.
fn argument_error_text(mut error: clap::Error) -> String {
    let contexts: Vec<_> = error
        .context()
        .filter_map(|(kind, value)| {
            if kind == ContextKind::Usage {
                return None;
            }
            let escape = |text: &str| eml_protocol::DisplayText::new(text).to_string();
            let value = match value {
                ContextValue::String(text) => ContextValue::String(escape(text)),
                ContextValue::Strings(text) => {
                    ContextValue::Strings(text.iter().map(|v| escape(v)).collect())
                }
                ContextValue::StyledStr(text) => {
                    ContextValue::StyledStr(escape(&text.to_string()).into())
                }
                ContextValue::StyledStrs(text) => ContextValue::StyledStrs(
                    text.iter().map(|v| escape(&v.to_string()).into()).collect(),
                ),
                _ => return None,
            };
            Some((kind, value))
        })
        .collect();
    for (kind, value) in contexts {
        error.insert(kind, value);
    }
    error.to_string()
}
fn native_message(context: &str, error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("{context}: {} (OS error {code})\n", error.kind()),
        None => format!("{context}: {}\n", error.kind()),
    }
}
fn invalid_request(error: InvalidRequest, control: &RunControl) -> u8 {
    let message = match error {
        InvalidRequest::Paths(error) => format!("Invalid request: {error}\n"),
        InvalidRequest::WorkingDirectory(error) => {
            native_message("Cannot bind the captured working directory", &error)
        }
        InvalidRequest::OutputDirectory(error) => {
            format!(
                "{}Choose an existing, writable folder for --output-dir; create it first if needed.\n",
                native_message("Cannot use the output folder", &error)
            )
        }
        InvalidRequest::DryRunModeRequired => {
            "The read-only operation requires a dry-run request\n".into()
        }
    };
    diagnostic(&message, 2, control)
}
fn install_termination_handler(control: &RunControl) -> Result<(), ctrlc::Error> {
    let handler_control = control.clone();
    let mut already_stopped = false;
    // ctrlc invokes this on its dedicated thread, not inside the native signal
    // handler. Count delivered callbacks; standard Unix signals can coalesce.
    ctrlc::set_handler(move || {
        if already_stopped {
            std::process::exit(130);
        }
        already_stopped = true;
        handler_control.request_stop();
    })
}
fn document_request(
    location: OsString,
    working: PathBuf,
    control: &RunControl,
) -> Result<RunRequest, u8> {
    let bytes = match request_bytes(location, control) {
        Ok(bytes) => bytes,
        Err(InputError::Cancelled) => return Err(130),
        Err(InputError::TooLarge) => {
            return Err(diagnostic("Request document exceeds 8 MiB\n", 2, control));
        }
        Err(InputError::Io(error)) => {
            return Err(diagnostic(
                &native_message("Cannot read request document", &error),
                2,
                control,
            ));
        }
        Err(InputError::Setup(error)) => {
            return Err(diagnostic(
                &native_message("Cannot start request transport", &error),
                1,
                control,
            ));
        }
        Err(InputError::Disconnected) => {
            return Err(diagnostic(
                "Request transport ended before returning an EOF result\n",
                1,
                control,
            ));
        }
    };
    if control.is_stopped() {
        return Err(130);
    }
    let value = match decode_request(&bytes, working) {
        Ok(value) => value,
        Err(error) => {
            return Err(diagnostic(
                &format!("Invalid request document: {error}\n"),
                2,
                control,
            ));
        }
    };
    if control.is_stopped() {
        return Err(130);
    }
    Ok(value)
}
pub(super) fn entry() -> u8 {
    let control = RunControl::default();
    if install_termination_handler(&control).is_err() {
        return diagnostic(
            "Cannot install process-local termination handling\n",
            1,
            &control,
        );
    }
    let arguments = match Arguments::try_parse() {
        Ok(value) => value,
        Err(error) => {
            let display = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let text = error.to_string();
            return if display {
                if control.is_stopped() {
                    return 130;
                }
                match terminal(&mut io::stdout().lock(), &text) {
                    Ok(()) => 0,
                    Err(_) => 1,
                }
            } else {
                diagnostic(&argument_error_text(error), 2, &control)
            };
        }
    };
    if control.is_stopped() {
        return 130;
    }
    let working = match std::env::current_dir() {
        Ok(value) => value,
        Err(error) => {
            return diagnostic(
                &native_message("Cannot capture the working directory", &error),
                2,
                &control,
            );
        }
    };
    let request = if let Some(location) = arguments.request {
        match document_request(location, working, &control) {
            Ok(request) => request,
            Err(exit) => return exit,
        }
    } else {
        RunRequest {
            paths: arguments.paths,
            working_directory: working,
            output: arguments
                .output_directory
                .map_or(OutputLocation::BesideSource, OutputLocation::Directory),
            mode: if arguments.dry_run {
                RunMode::DryRun
            } else {
                RunMode::Create
            },
        }
    };
    if control.is_stopped() {
        return 130;
    }
    let result = match arguments.format {
        Format::Json => eml_core::run(&request, &control, &mut JsonSink::new(io::stdout().lock())),
        Format::Human => {
            eml_core::run(&request, &control, &mut HumanSink::new(io::stdout().lock()))
        }
    };
    match result {
        Ok(outcome) => outcome.exit_class().code(),
        Err(error) => invalid_request(error, &control),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn native_arguments_are_preserved_and_request_conflicts_are_presence_based() {
        let args =
            Arguments::try_parse_from(["tool", "--request", "-", "--format", "json"]).unwrap();
        assert!(matches!(args.format, Format::Json));
        assert_eq!(args.request, Some("-".into()));
        assert!(Arguments::try_parse_from(["tool"]).is_err());
        assert!(Arguments::try_parse_from(["tool", "--dry-run"]).is_err());
        #[cfg(unix)]
        let native = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"mail\xff.eml".to_vec())
        };
        #[cfg(windows)]
        let native = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x61, 0xd800, 0x2e, 0x65, 0x6d, 0x6c])
        };
        let args = Arguments::try_parse_from([
            OsString::from("tool"),
            OsString::from("--"),
            native.clone(),
        ])
        .unwrap();
        assert_eq!(args.paths, [PathBuf::from(native)]);
    }
    #[test]
    fn request_limit_requires_eof_and_rejects_one_more_byte_with_bounded_reads() {
        struct Reader {
            bytes: usize,
            calls: usize,
            eof: bool,
        }
        impl Read for Reader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert!(buffer.len() <= 65_536);
                self.calls += 1;
                let count = buffer.len().min(self.bytes);
                self.eof |= count == 0;
                buffer[..count].fill(b' ');
                self.bytes -= count;
                Ok(count)
            }
        }
        let mut exact = Reader {
            bytes: 8_388_608,
            calls: 0,
            eof: false,
        };
        let bytes = bounded_read(&mut exact, &RunControl::default()).unwrap();
        assert_eq!(bytes.len(), 8_388_608);
        assert!(
            exact.eof,
            "Exact limit still requires an actual EOF observation"
        );
        assert!(matches!(
            bounded_read(
                Reader {
                    bytes: 8_388_609,
                    calls: 0,
                    eof: false,
                },
                &RunControl::default()
            ),
            Err(InputError::TooLarge)
        ));
    }
    #[test]
    fn request_native_read_error_and_delivered_stop_are_distinct_from_eof() {
        struct Failure;
        impl Read for Failure {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from_raw_os_error(5))
            }
        }
        let Err(InputError::Io(error)) = bounded_read(Failure, &RunControl::default()) else {
            panic!("Native read error was hidden");
        };
        assert_eq!(error.raw_os_error(), Some(5));
        let control = RunControl::default();
        control.request_stop();
        assert!(matches!(
            bounded_read(Cursor::new(b"{}"), &control),
            Err(InputError::Cancelled)
        ));
    }
    #[test]
    fn short_reads_and_interrupted_retry_preserve_document_bytes() {
        struct Short {
            offset: usize,
            retried: bool,
        }
        impl Read for Short {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.retried {
                    self.retried = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let input = b"{\"protocol\":1}";
                let count = 2.min(buffer.len()).min(input.len() - self.offset);
                buffer[..count].copy_from_slice(&input[self.offset..self.offset + count]);
                self.offset += count;
                Ok(count)
            }
        }
        assert_eq!(
            bounded_read(
                Short {
                    offset: 0,
                    retried: false
                },
                &RunControl::default()
            )
            .unwrap(),
            b"{\"protocol\":1}"
        );
    }
}
