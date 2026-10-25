//! Deliberately false owned-fixture processors, never distributed.
#![forbid(unsafe_code)]
use std::{
    fs,
    io::{Read, Write},
    process::ExitCode,
};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        println!(
            "emlattachmentremover {}",
            eml_core::product::VERSION_WITH_BUILD.as_str()
        );
        return Ok(());
    }
    if args != ["--format", "json", "--", "a.eml"] {
        return Err("Counterfeit fixture accepts only its owned smoke invocation".into());
    }
    let directory = std::env::current_dir()?;
    let parent = directory.parent().ok_or("Missing owned fixture parent")?;
    if fs::read(parent.join("owned-counterfeit-marker"))?
        != b"EML owned synthetic counterfeit cases\n"
        || fs::read("a.eml")? != INPUT
    {
        return Err("Refusing non-synthetic/unmarked counterfeit input".into());
    }
    let mode = std::env::var("EML_ARCHIVE_COUNTERFEIT_MODE")?;
    if !matches!(mode.as_str(), "identity-copy" | "summary-only") {
        return Err("Unknown owned counterfeit mode".into());
    }
    let mut trace = Vec::new();
    fs::File::open(parent.join(format!("counterfeit-{mode}-trace.jsonl")))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut trace)?;
    if trace.len() > 1024 * 1024 || trace.last() != Some(&b'\n') {
        return Err("Invalid bounded counterfeit trace".into());
    }
    let output = match mode.as_str() {
        "identity-copy" => {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open("a.no-attachments.eml")?;
            output.write_all(INPUT)?;
            output.flush()?;
            trace
        }
        "summary-only" => {
            let line = trace[..trace.len() - 1]
                .rsplit(|byte| *byte == b'\n')
                .next()
                .ok_or("Missing counterfeit summary")?;
            let mut output = line.to_vec();
            output.push(b'\n');
            output
        }
        _ => return Err("Unknown owned counterfeit mode".into()),
    };
    std::io::stdout().write_all(&output)?;
    std::io::stdout().flush()?;
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
