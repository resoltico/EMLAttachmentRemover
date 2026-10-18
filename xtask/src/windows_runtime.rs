//! PE import policy; structural evidence is separate from execution/static inputs.
use crate::Result;
use object::{
    Object,
    endian::LittleEndian as LE,
    read::pe::{ImageNtHeaders, ImageOptionalHeader, Import, PeFile64},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn library(bytes: &[u8]) -> Result<String> {
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(b))
    {
        return Err("PE library is not a bounded ordinary ASCII DLL basename".into());
    }
    let name = std::str::from_utf8(bytes)?.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "kernel32.dll"
            | "ntdll.dll"
            | "advapi32.dll"
            | "userenv.dll"
            | "ws2_32.dll"
            | "bcrypt.dll"
            | "bcryptprng.dll"
            | "secur32.dll"
            | "crypt32.dll"
            | "ole32.dll"
            | "shell32.dll"
            | "user32.dll"
            | "ucrtbase.dll"
            | "api-ms-win-core-synch-l1-2-0.dll"
            | "api-ms-win-core-path-l1-1-0.dll"
            | "api-ms-win-crt-conio-l1-1-0.dll"
            | "api-ms-win-crt-convert-l1-1-0.dll"
            | "api-ms-win-crt-environment-l1-1-0.dll"
            | "api-ms-win-crt-filesystem-l1-1-0.dll"
            | "api-ms-win-crt-heap-l1-1-0.dll"
            | "api-ms-win-crt-locale-l1-1-0.dll"
            | "api-ms-win-crt-math-l1-1-0.dll"
            | "api-ms-win-crt-multibyte-l1-1-0.dll"
            | "api-ms-win-crt-private-l1-1-0.dll"
            | "api-ms-win-crt-process-l1-1-0.dll"
            | "api-ms-win-crt-runtime-l1-1-0.dll"
            | "api-ms-win-crt-stdio-l1-1-0.dll"
            | "api-ms-win-crt-string-l1-1-0.dll"
            | "api-ms-win-crt-time-l1-1-0.dll"
            | "api-ms-win-crt-utility-l1-1-0.dll"
    ) {
        return Err(format!("Unreviewed Windows runtime DLL: {name}").into());
    }
    Ok(name)
}
fn symbol(import: Import<'_>, count: &mut usize) -> Result<()> {
    *count += 1;
    if *count > 65536 {
        return Err("PE import function limit exceeded".into());
    }
    if let Import::Name(_, name) = import
        && (name.is_empty() || name.len() > 4096)
    {
        return Err("PE function name is empty or exceeds its bound".into());
    }
    Ok(())
}
pub(crate) fn inspect(bytes: &[u8], architecture: &str) -> Result<Value> {
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("PE image exceeds its bound".into());
    }
    let file = PeFile64::parse(bytes)?;
    let actual = match file.architecture() {
        object::Architecture::X86_64 => "x86_64",
        object::Architecture::Aarch64 => "aarch64",
        _ => return Err("Unsupported PE CPU".into()),
    };
    if actual != architecture || file.kind() != object::ObjectKind::Executable {
        return Err("PE is not the requested native executable CPU/type".into());
    }
    let optional = file.nt_headers().optional_header();
    if optional.subsystem() != object::pe::IMAGE_SUBSYSTEM_WINDOWS_CUI {
        return Err("Windows CLI is not a console-subsystem executable".into());
    }
    let mut functions = 0usize;
    let mut ordinary = BTreeMap::new();
    if let Some(table) = file.import_table()? {
        let mut descriptors = table.descriptors()?;
        let mut count = 0;
        while let Some(descriptor) = descriptors.next()? {
            count += 1;
            if count > 256 {
                return Err("PE import descriptor limit exceeded".into());
            }
            let name = library(table.name(descriptor.name.get(LE))?)?;
            let start = functions;
            let lookup = descriptor.original_first_thunk.get(LE);
            let mut thunks = table.thunks(if lookup == 0 {
                descriptor.first_thunk.get(LE)
            } else {
                lookup
            })?;
            while let Some(thunk) = thunks.next::<object::pe::ImageNtHeaders64>()? {
                symbol(
                    table.import::<object::pe::ImageNtHeaders64>(thunk)?,
                    &mut functions,
                )?;
            }
            if functions == start {
                return Err("PE import descriptor has no functions".into());
            }
            *ordinary.entry(name).or_insert(0usize) += functions - start;
        }
    }
    let mut delayed = BTreeMap::new();
    if let Some(table) = file
        .data_directories()
        .delay_load_import_table(bytes, &file.section_table())?
    {
        let mut descriptors = table.descriptors()?;
        let mut count = 0;
        while let Some(descriptor) = descriptors.next()? {
            count += 1;
            if count > 256 {
                return Err("PE delayed import descriptor limit exceeded".into());
            }
            if descriptor.attributes.get(LE) != 1 {
                return Err("Unsupported PE delayed import address mode".into());
            }
            let name = library(table.name(descriptor.dll_name_rva.get(LE))?)?;
            let start = functions;
            let mut thunks = table.thunks(descriptor.import_name_table_rva.get(LE))?;
            while let Some(thunk) = thunks.next::<object::pe::ImageNtHeaders64>()? {
                symbol(
                    table.import::<object::pe::ImageNtHeaders64>(thunk)?,
                    &mut functions,
                )?;
            }
            if functions == start {
                return Err("PE delayed descriptor has no functions".into());
            }
            *delayed.entry(name).or_insert(0usize) += functions - start;
        }
    }
    if functions == 0 {
        return Err("PE runtime dependency discovery is unexpectedly empty".into());
    }
    Ok(
        json!({"image":xtask::archive::ByteIdentity::of(bytes),"architecture":actual,"ordinary_imports":ordinary,"delay_imports":delayed,"functions":functions,
        "scope":"PE64 CPU/console/ordinary and delayed system-UCRT import structure; no native execution or final static-link provenance claim"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    // Hand-authored PE64 layout: one .idata section, one named ordinary import
    // and optional ordinal delay import. Never executed or used as a product.
    fn image(normal: &str, delay: Option<&str>) -> Vec<u8> {
        fn word(b: &mut [u8], p: usize, v: u16) {
            b[p..p + 2].copy_from_slice(&v.to_le_bytes());
        }
        fn dword(b: &mut [u8], p: usize, v: u32) {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn qword(b: &mut [u8], p: usize, v: u64) {
            b[p..p + 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut b = vec![0u8; 1024];
        b[..2].copy_from_slice(b"MZ");
        dword(&mut b, 60, 128);
        b[128..132].copy_from_slice(b"PE\0\0");
        word(&mut b, 132, 0x8664);
        word(&mut b, 134, 1);
        word(&mut b, 148, 240);
        word(&mut b, 150, 0x22);
        let o = 152;
        word(&mut b, o, 0x20b);
        dword(&mut b, o + 16, 0x1000);
        qword(&mut b, o + 24, 0x140000000);
        dword(&mut b, o + 32, 0x1000);
        dword(&mut b, o + 36, 512);
        dword(&mut b, o + 56, 0x2000);
        dword(&mut b, o + 60, 512);
        word(&mut b, o + 68, 3);
        dword(&mut b, o + 108, 16);
        dword(&mut b, o + 120, 0x1000);
        dword(&mut b, o + 124, 40);
        let s = o + 240;
        b[s..s + 6].copy_from_slice(b".idata");
        dword(&mut b, s + 8, 512);
        dword(&mut b, s + 12, 0x1000);
        dword(&mut b, s + 16, 512);
        dword(&mut b, s + 20, 512);
        dword(&mut b, s + 36, 0x40000040);
        dword(&mut b, 512, 0x10c0);
        dword(&mut b, 524, 0x1100);
        dword(&mut b, 528, 0x10c0);
        qword(&mut b, 704, 0x1140);
        b[768..768 + normal.len()].copy_from_slice(normal.as_bytes());
        b[834..847].copy_from_slice(b"GetLastError\0");
        if let Some(delay) = delay {
            dword(&mut b, o + 112 + 13 * 8, 0x1080);
            dword(&mut b, o + 116 + 13 * 8, 64);
            dword(&mut b, 640, 1);
            dword(&mut b, 644, 0x1180);
            dword(&mut b, 656, 0x11e0);
            dword(&mut b, 652, 0x11e0);
            b[896..896 + delay.len()].copy_from_slice(delay.as_bytes());
            qword(&mut b, 992, (1u64 << 63) | 12);
        }
        b
    }
    #[test]
    fn pe_import_policy_checks_both_tables_cpu_and_basename() {
        let valid = image("KERNEL32.dll", Some("ucrtbase.dll"));
        let result = inspect(&valid, "x86_64").unwrap();
        assert_eq!(result["functions"], 2);
        assert_eq!(result["delay_imports"]["ucrtbase.dll"], 1);
        assert!(inspect(&valid, "aarch64").is_err());
        assert!(
            inspect(&image("libunwind.dll", None), "x86_64")
                .unwrap_err()
                .to_string()
                .contains("Unreviewed")
        );
        assert!(
            inspect(&image("KERNEL32.dll", Some("VCRUNTIME140.dll")), "x86_64")
                .unwrap_err()
                .to_string()
                .contains("Unreviewed")
        );
        assert!(inspect(&image("C:\\kernel32.dll", None), "x86_64").is_err());
        let mut bad = valid;
        bad[152 + 112 + 13 * 8..152 + 116 + 13 * 8].copy_from_slice(&0xfffffff0u32.to_le_bytes());
        assert!(inspect(&bad, "x86_64").is_err());
    }
}
