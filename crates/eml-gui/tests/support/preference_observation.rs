//! Nonshipping real Foundation preferences in a marker-owned fresh suite.
#![allow(
    unsafe_code,
    reason = "Typed property-list fixture values and the shared Foundation preference boundary"
)]
#[path = "../../src/preferences.rs"]
#[allow(
    dead_code,
    reason = "The probe uses an owned suite; production load uses the standard application domain"
)]
mod preferences;
use base64::{Engine, engine::general_purpose::STANDARD};
use objc2::{AnyThread, rc::Retained};
use objc2_foundation::{NSNumber, NSString, NSUserDefaults};
use std::{
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
};
const MARKER: &str = "FixtureOwnership";
const KEY: &str = "copyDestinationBytes";
fn argument<'a>(arguments: &'a [String], key: &str) -> &'a str {
    arguments
        .iter()
        .position(|argument| argument == key)
        .and_then(|index| arguments.get(index + 1))
        .expect("Required fixture argument")
}
fn snapshot(defaults: &NSUserDefaults, suite: &NSString, marker: &NSString) -> String {
    let Some(domain) = defaults.persistentDomainForName(suite) else {
        return "has_saved_keys:false\nowner:false\ncount:0\nkind:none\nvalue:\n".into();
    };
    // Foundation may retain an empty dictionary/cache after domain removal.
    // Cleanup proves no persisted keys, not removal of an OS cache file.
    if domain.is_empty() {
        return "has_saved_keys:false\nowner:false\ncount:0\nkind:none\nvalue:\n".into();
    }
    let owner = domain
        .objectForKey(&NSString::from_str(MARKER))
        .and_then(|object| object.downcast::<NSString>().ok())
        .is_some_and(|value| value.isEqualToString(marker));
    let mut kind = "missing";
    let mut value = String::new();
    if let Some(object) = domain.objectForKey(&NSString::from_str(KEY)) {
        if let Some(text) = object.downcast_ref::<NSString>() {
            kind = "string";
            let length = text.length();
            if length <= 512 {
                let units: Vec<_> = (0..length)
                    .map(|index| text.characterAtIndex(index))
                    .collect();
                if units.iter().all(|unit| *unit <= 127) {
                    value = STANDARD
                        .encode(units.into_iter().map(|unit| unit as u8).collect::<Vec<_>>());
                } else {
                    kind = "non_ascii_string";
                }
            } else {
                kind = "oversized_string";
            }
        } else if let Some(number) = object.downcast_ref::<NSNumber>() {
            kind = "number";
            value = STANDARD.encode(number.intValue().to_string());
        } else {
            kind = "other";
        }
    }
    format!(
        "has_saved_keys:true\nowner:{owner}\ncount:{}\nkind:{kind}\nvalue:{value}\n",
        domain.len()
    )
}
fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    let suite_name = argument(&arguments, "--owned-suite");
    let marker_text = argument(&arguments, "--marker");
    assert!(suite_name.starts_with("io.github.resoltico.eml-preference-observation."));
    assert!(suite_name.len() <= 256 && marker_text.len() <= 256);
    let suite = NSString::from_str(suite_name);
    let marker = NSString::from_str(marker_text);
    let defaults =
        NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&suite)).unwrap();
    println!("scope:{suite_name}:{marker_text}");
    match argument(&arguments, "--action") {
        "seed-string" | "seed-number" => {
            assert!(
                defaults
                    .persistentDomainForName(&suite)
                    .is_none_or(|domain| domain.is_empty()),
                "Suite must be new and empty"
            );
            let key = NSString::from_str(KEY);
            // SAFETY: only this fresh test suite; NSString/NSNumber are valid
            // immutable property-list values. Write the ownership marker first.
            unsafe {
                defaults.setObject_forKey(Some(&marker), &NSString::from_str(MARKER));
            }
            if argument(&arguments, "--action") == "seed-string" {
                let value = argument(&arguments, "--value");
                assert!(value.len() <= 512);
                unsafe {
                    defaults.setObject_forKey(Some(&NSString::from_str(value)), &key);
                }
            } else {
                unsafe {
                    defaults.setObject_forKey(Some(&NSNumber::new_i32(7)), &key);
                }
            }
            print!("{}", snapshot(&defaults, &suite, &marker));
        }
        "observe" => print!("{}", snapshot(&defaults, &suite, &marker)),
        "choose" => {
            print!("before-load\n{}", snapshot(&defaults, &suite, &marker));
            let mut preference =
                preferences::DestinationPreference::from_defaults(Retained::clone(&defaults));
            print!("after-load\n{}", snapshot(&defaults, &suite, &marker));
            println!("session-only:{}", preference.session_only());
            match preference.destination() {
                Ok(None) => println!("destination:beside"),
                Ok(Some(path)) => println!(
                    "destination:{}",
                    STANDARD.encode(path.as_os_str().as_bytes())
                ),
                Err(_) => println!("destination:invalid"),
            }
            preference
                .choose(Some(PathBuf::from(std::ffi::OsString::from_vec(
                    b"/owned-synthetic/changed-\xff".to_vec(),
                ))))
                .unwrap();
            print!("after-choice\n{}", snapshot(&defaults, &suite, &marker));
        }
        "cleanup" => {
            if let Some(domain) = defaults.persistentDomainForName(&suite) {
                let owner = domain
                    .objectForKey(&NSString::from_str(MARKER))
                    .and_then(|object| object.downcast::<NSString>().ok())
                    .is_some_and(|value| value.isEqualToString(&marker));
                assert!(
                    domain.is_empty() || owner,
                    "Refuse to remove a nonempty suite without the exact ownership marker"
                );
                defaults.removePersistentDomainForName(&suite);
            }
            println!("cleanup:requested");
        }
        _ => panic!("Unknown preference observation action"),
    }
}
