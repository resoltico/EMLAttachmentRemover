#![no_main]
#![forbid(unsafe_code)]
use eml_core::{PreparationError, Refusal, RunControl, prepare_message};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    // Every success has already executed the independent runtime source authorizer
    // and byte/structure verifier. A caught internal mismatch is a fuzz finding.
    match prepare_message(input, &RunControl::default()) {
        Ok(prepared) => {
            assert!(prepared.as_bytes().len() <= input.len());
            let mut source = input.iter();
            for byte in prepared.as_bytes() {
                assert!(
                    source.any(|original| original == byte),
                    "Output is not a source subsequence"
                );
            }
            assert!(prepared.removed().len() <= 10_000);
            for removal in prepared.removed() {
                assert!(removal.section.len() <= 383);
                assert!(removal.content_type.len() <= 255 && removal.content_type.is_ascii());
                assert!(removal.bytes <= input.len() as u64);
                if let Some(name) = &removal.filename {
                    assert!(name.chars().count() <= 255);
                    assert!(!name.chars().any(|c| matches!(c as u32,0..=31 | 127..=159)));
                }
            }
            let again = prepare_message(prepared.as_bytes(), &RunControl::default())
                .expect("Successful preparation is not a fixed point");
            assert_eq!(again.as_bytes(), prepared.as_bytes());
            assert!(again.removed().is_empty() && again.stripped_fields().is_empty());
        }
        Err(PreparationError::Refused {
            kind: Refusal::Internal,
            ..
        }) => panic!("Independent MIME verification detected an internal mismatch"),
        Err(PreparationError::Interrupted) => {
            panic!("A fresh pure-preparation control was interrupted")
        }
        Err(PreparationError::Refused { .. }) => (),
    }
});
