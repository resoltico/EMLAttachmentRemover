//! Controller ownership and native feedback contracts.
use super::*;
#[test]
fn stale_native_opener_feedback_cannot_replace_a_later_batch_notice() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
        root.path().join("absent-cli"),
        root.path().to_owned(),
        preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    c.open_advisory(OpenTarget::Main, "stale opener error", move || {
        rx.recv().unwrap();
        Err("failed".into())
    });
    c.stage(vec![root.path().join("next.eml")]).unwrap();
    c.message("current batch notice");
    tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !c.open_jobs.borrow().is_empty() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        c.notice.borrow().as_ref().unwrap().0,
        "current batch notice"
    );
}
#[test]
fn quit_waits_for_owned_report_writer_and_invalidates_late_panels() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
        root.path().join("absent"),
        root.path().to_owned(),
        preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        rx.recv().unwrap();
        Ok(())
    });
    c.runtime.borrow_mut().export = Some(ExportJob {
        path: root.path().join("report"),
        task,
    });
    let epoch = c.panel_epoch();
    assert!(!c.request_quit());
    assert_ne!(epoch, c.panel_epoch());
    c.stop_and_quit();
    c.tick();
    assert!(!c.should_exit());
    tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !c.should_exit() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!c.is_busy());
}

#[test]
fn theme_tokens_keep_readable_contrast_in_both_schemes() {
    use std::collections::BTreeMap;
    fn luminance(rgb: u32) -> f64 {
        let channels = [rgb >> 16, rgb >> 8, rgb];
        channels
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(v, weight)| {
                let c = f64::from(v & 255) / 255.0;
                weight
                    * if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }
    fn ratio(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    assert_eq!(ratio(0, 0xffffff), 21.0);
    assert_eq!(ratio(0x777777, 0x777777), 1.0);
    let tokens: BTreeMap<_, Vec<_>> = include_str!("../ui/theme.slint")
        .lines()
        .filter(|line| line.contains("<color>"))
        .map(|line| {
            let name = line
                .split('>')
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .trim();
            let colors = line
                .split('#')
                .skip(1)
                .map(|s| u32::from_str_radix(&s[..6], 16).unwrap())
                .collect();
            (name, colors)
        })
        .collect();
    for background in ["sidebar", "surface", "raised", "accent-soft"] {
        for foreground in ["text", "muted", "accent", "ok", "warn", "bad", "idle"] {
            assert_eq!(tokens[foreground].len(), 2);
            assert_eq!(tokens[background].len(), 2);
            for (scheme, (&front, &back)) in tokens[foreground]
                .iter()
                .zip(&tokens[background])
                .enumerate()
            {
                let contrast = ratio(front, back);
                assert!(
                    contrast >= 4.5,
                    "{foreground} on {background}, scheme {scheme}: {contrast}"
                );
            }
        }
    }
}

#[test]
fn native_open_remains_nonblocking_scoped_and_clears_recovered_errors() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
        root.path().join("unused-cli"),
        root.path().to_owned(),
        preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    for target in [OpenTarget::About, OpenTarget::Licenses] {
        let (release, wait) = std::sync::mpsc::channel();
        c.open_advisory(target, "Cannot open notices. Try again.", move || {
            wait.recv().unwrap();
            Err("missing fixture".into())
        });
        c.message("A separate operation completed.");
        let notice = match target {
            OpenTarget::About => c.about.get_notice(),
            OpenTarget::Licenses => c.licenses.get_notice(),
            OpenTarget::Main => unreachable!(),
        };
        assert_eq!(notice, "Opening…");
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !c.open_jobs.borrow().is_empty() {
            c.tick();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let (notice, detail) = match target {
            OpenTarget::About => (c.about.get_notice(), c.about.get_notice_details()),
            OpenTarget::Licenses => (c.licenses.get_notice(), c.licenses.get_notice_details()),
            OpenTarget::Main => unreachable!(),
        };
        assert_eq!(notice, "Cannot open notices. Try again.");
        assert_eq!(detail, "missing fixture");
        assert_eq!(c.ui.get_view().notice, "A separate operation completed.");
        c.open_advisory(target, "Cannot open notices. Try again.", || Ok(()));
        while !c.open_jobs.borrow().is_empty() {
            c.tick();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let (notice, detail) = match target {
            OpenTarget::About => (c.about.get_notice(), c.about.get_notice_details()),
            OpenTarget::Licenses => (c.licenses.get_notice(), c.licenses.get_notice_details()),
            OpenTarget::Main => unreachable!(),
        };
        assert_eq!(notice, "Open command sent.");
        assert!(detail.is_empty());
    }
    let (release, wait) = std::sync::mpsc::channel();
    c.open_advisory(OpenTarget::Main, "Cannot reveal.", move || {
        wait.recv().unwrap();
        Err("obsolete error".into())
    });
    c.message("Newer operation.");
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !c.open_jobs.borrow().is_empty() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(c.ui.get_view().notice, "Newer operation.");
}

#[test]
fn attachment_labels_preserve_resource_kind_and_unnamed_media_type() {
    use eml_protocol::records::{Nullable, Removal, RemovedRecord};
    for (content_type, reason, expected) in [
        ("image/png", Removal::RelatedResource, "Inline image"),
        ("Image/JPEG", Removal::RelatedResource, "Inline image"),
        ("text/css", Removal::RelatedResource, "Inline resource"),
        ("font/woff", Removal::RelatedResource, "Inline resource"),
        ("Text/Calendar", Removal::NonBodyPart, "Calendar invite"),
    ] {
        let part = RemovedRecord {
            section: "2".into(),
            content_type: content_type.into(),
            filename: Nullable::Value(String::new()),
            bytes: 100,
            reason,
        };
        let view = wording::attachment(&part);
        assert_eq!(view.name, content_type);
        assert_eq!(view.kind, expected);
        assert_eq!(view.size, "100 B");
    }
}

#[test]
fn t1_settlement_without_busy_work_is_rendered_on_next_tick() {
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("check.sh");
    let records = include_str!("../../../tests/contract/protocol/planned-create-unix.jsonl")
        .replace("@VERSION@", eml_core::product::VERSION);
    std::fs::write(
        &cli,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'RECORDS'\n{records}\nRECORDS\n"),
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let c = DesktopController::new(
        cli,
        root.path().into(),
        preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    c.stage(vec![root.path().join("mail.eml")]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut r = c.runtime.borrow_mut();
        let a = r.active.as_mut().unwrap();
        if let Some(completion) = a.worker.poll().unwrap() {
            let (kind, generation) = (a.kind, a.generation);
            r.batch.settle(kind, generation, completion, false);
            r.active = None;
            break;
        }
        assert!(Instant::now() < deadline);
        drop(r);
        std::thread::yield_now();
    }
    assert!(!c.is_busy());
    c.tick();
    assert_eq!(c.window().get_view().title, "1 email");
    assert_eq!(c.window().get_rows().row_count(), 1);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "No attachments"
    );
}
