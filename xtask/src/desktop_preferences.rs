//! Compare complete native observations, with a picker-scenario-only OS policy.
use crate::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn compare(before: &Value, after: &Value, real_pickers_observed: bool) -> Result<Value> {
    let before = before
        .as_object()
        .ok_or("Preference before snapshot is not an object")?;
    let after = after
        .as_object()
        .ok_or("Preference after snapshot is not an object")?;
    let keys = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();
    let mut permitted = Vec::new();
    for key in keys {
        let old = before.get(key);
        let current = after.get(key);
        if old == current {
            continue;
        }
        let expected_type = match key.as_str() {
            "NSNavPanelExpandedSizeForOpenMode"
            | "NSWindow Frame GoToSheet"
            | "NSWindow Frame NSNavPanelAutosaveName" => Some("string"),
            "NSOSPLastRootDirectory" => Some("data"),
            "NSNavLastUserSetHideExtensionButtonState" => Some("boolean"),
            _ => None,
        };
        let allowed = real_pickers_observed
            && expected_type.is_some_and(|expected| {
                let matches_type = |value: &Value| {
                    value["type"] == expected
                        && match expected {
                            "string" => value["value"].is_string(),
                            "boolean" => value["value"].is_boolean(),
                            "data" => value["base64"].is_string(),
                            _ => false,
                        }
                };
                current.is_some_and(matches_type) && old.is_none_or(matches_type)
            });
        if !allowed {
            return Err(format!(
                "Persistent preference changed outside the proved picker-state policy: {key}"
            )
            .into());
        }
        permitted
            .push(json!({"key":key,"before_present":old.is_some(),"native_type":expected_type}));
    }
    Ok(
        json!({"whole_domain_unchanged":before==after,"application_and_other_preferences_unchanged":true,"real_pickers_observed":real_pickers_observed,"permitted_system_changes":permitted}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_picker_keys_require_the_actual_scenario_and_exact_types() {
        let empty = json!({});
        let os = json!({"NSNavPanelExpandedSizeForOpenMode":{"type":"string","value":"{880,448}"},"NSOSPLastRootDirectory":{"type":"data","base64":"AAH/"},"NSWindow Frame GoToSheet":{"type":"string","value":"owned frame"},"NSNavLastUserSetHideExtensionButtonState":{"type":"boolean","value":false},"NSWindow Frame NSNavPanelAutosaveName":{"type":"string","value":"112 179 800 448 0 0 1024 743 "}});
        assert!(compare(&empty, &os, false).is_err());
        let result = compare(&empty, &os, true).unwrap();
        assert_eq!(result["whole_domain_unchanged"], false);
        assert_eq!(
            result["permitted_system_changes"].as_array().unwrap().len(),
            5
        );
        assert_eq!(
            compare(&empty, &empty, false).unwrap()["whole_domain_unchanged"],
            true
        );
        for key in [
            "NSNavPanelExpandedSizeForOpenMode",
            "NSOSPLastRootDirectory",
            "NSWindow Frame GoToSheet",
            "NSNavLastUserSetHideExtensionButtonState",
            "NSWindow Frame NSNavPanelAutosaveName",
        ] {
            let single = json!({(key):os[key]});
            assert!(compare(&empty, &single, false).is_err());
            assert!(compare(&empty, &single, true).is_ok());
            let mut changed = os.clone();
            changed[key] = json!({"type":"integer","value":"1"});
            assert!(compare(&empty, &changed, true).is_err());
            assert!(compare(&changed, &os, true).is_err());
            let mut removed = os.clone();
            removed.as_object_mut().unwrap().remove(key);
            assert!(compare(&os, &removed, true).is_err());
        }
        assert!(compare(&os, &empty, true).is_err());
        for (key, wrong) in [
            (
                "NSNavLastUserSetHideExtensionButtonState",
                json!({"type":"boolean","value":0}),
            ),
            (
                "NSNavLastUserSetHideExtensionButtonState",
                json!({"type":"boolean","value":"false"}),
            ),
            (
                "NSWindow Frame NSNavPanelAutosaveName",
                json!({"type":"string","value":0}),
            ),
        ] {
            assert!(compare(&empty, &json!({(key):wrong}), true).is_err());
        }
    }
    #[test]
    fn destination_unknown_presence_value_and_type_changes_are_never_exempted() {
        for key in [
            "copyDestinationBytes",
            "unrecognizedSetting",
            "NSUnreviewedSetting",
        ] {
            let before = json!({(key):{"type":"string","value":"owned-before"}});
            assert!(before.get(key).is_some());
            assert!(compare(&before, &before, true).is_ok());
            for after in [
                json!({}),
                json!({(key):{"type":"string","value":"owned-after"}}),
                json!({(key):{"type":"data","base64":"AAH/"}}),
            ] {
                assert!(compare(&before, &after, true).is_err());
                assert!(compare(&json!({}), &after, true).is_err() || after == json!({}));
            }
        }
    }
}
