//! Structural metadata planning on actual owned files, without executing MIME/copies.
use eml_core::{
    DestinationObservation, DestinationPlanState, ErrorCode, OutputLocation, RunControl, RunMode,
    RunRequest, collect_sources, plan_destinations,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn scratch() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/destination-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn request(root: &Path, paths: &[&str], output: Option<&str>) -> RunRequest {
    RunRequest {
        paths: paths.iter().map(PathBuf::from).collect(),
        working_directory: root.to_owned(),
        output: output.map_or(OutputLocation::BesideSource, |path| {
            OutputLocation::Directory(PathBuf::from(path))
        }),
        mode: RunMode::DryRun,
    }
}

#[test]
fn explicit_files_are_flat_and_walked_files_keep_root_and_relative_layout() {
    let root = scratch();
    fs::create_dir_all(root.path().join("Inbox/nested")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    for path in ["file.BIN", "Inbox/a.eml", "Inbox/nested/b.EML"] {
        fs::write(root.path().join(path), b"synthetic not MIME").unwrap();
    }
    let req = request(root.path(), &["file.BIN", "Inbox"], Some("out"));
    let sources = collect_sources(&req, &RunControl::default()).unwrap();
    let source_bytes = sources.source_path_bytes();
    let plan = plan_destinations(sources, &RunControl::default());
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert_eq!(plan.records().len(), 3);
    let expected = [
        "out/Inbox.eml-dir/a.jqp3zjytiqvs4hdhcvxvgec6qa.no-attachments.eml",
        "out/Inbox.eml-dir/nested.eml-dir/b.6bygukm4ll2wnnucyuby6ydq2q.no-attachments.eml",
        "out/file.BIN.b7evpv4zhqpz2fthdcg5vw7ldu.no-attachments.eml",
    ]
    .map(PathBuf::from);
    assert_eq!(
        (0..plan.records().len())
            .map(|index| plan.display_path(index).unwrap().into_owned())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(plan.records().iter().all(|row| row.failure().is_none()));
    assert_eq!(
        plan.records()[2].destination().unwrap().observation(),
        DestinationObservation::Missing
    );
    assert_eq!(
        plan.records()[1].destination().unwrap().observation(),
        DestinationObservation::MissingParent
    );
    assert!(plan.reported_path_bytes() > source_bytes);
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    assert_eq!(
        fs::read(root.path().join("file.BIN")).unwrap(),
        b"synthetic not MIME"
    );
    drop(plan);
    root.close().unwrap();
}

#[test]
fn collisions_mark_all_contenders_even_when_existing_type_already_failed() {
    let root = scratch();
    fs::create_dir(root.path().join("left")).unwrap();
    fs::create_dir(root.path().join("right")).unwrap();
    fs::create_dir_all(
        root.path()
            .join("out/a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"),
    )
    .unwrap();
    fs::write(root.path().join("left/a.eml"), b"left").unwrap();
    fs::write(root.path().join("right/a.eml"), b"right").unwrap();
    let req = request(root.path(), &["left/a.eml", "right/a.eml"], Some("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert_eq!(plan.records().len(), 2);
    for record in plan.records() {
        assert_eq!(
            record.failure().unwrap().code(),
            ErrorCode::DestinationCollision
        );
        assert!(
            matches!(record.destination().unwrap().observation(), DestinationObservation::Present(value) if value.kind == eml_fs::EntryKind::Directory)
        );
    }
    assert!(
        root.path()
            .join("out/a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
            .is_dir()
    );
    drop(plan);
    root.close().unwrap();
}

#[test]
fn missing_mirror_tail_reservations_use_the_bound_prefix_for_all_contenders() {
    let root = scratch();
    for parent in ["left", "right"] {
        fs::create_dir_all(root.path().join(parent).join("Inbox")).unwrap();
        fs::write(root.path().join(parent).join("Inbox/a.eml"), b"owned").unwrap();
    }
    fs::create_dir(root.path().join("out")).unwrap();
    let req = request(root.path(), &["left/Inbox", "right/Inbox"], Some("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert_eq!(plan.records().len(), 2);
    for (index, record) in plan.records().iter().enumerate() {
        assert_eq!(
            record.failure().unwrap().code(),
            ErrorCode::DestinationCollision
        );
        assert_eq!(
            record.destination().unwrap().observation(),
            DestinationObservation::MissingParent
        );
        assert_eq!(
            plan.display_path(index).unwrap().as_ref(),
            Path::new("out/Inbox.eml-dir/a.jqp3zjytiqvs4hdhcvxvgec6qa.no-attachments.eml")
        );
    }
    assert!(!root.path().join("out/Inbox.eml-dir").exists());
    drop(plan);
    root.close().unwrap();
}

#[test]
fn source_reservations_include_explicit_generated_names_and_hardlink_identities() {
    let root = scratch();
    fs::write(root.path().join("a.eml"), b"a").unwrap();
    fs::write(
        root.path()
            .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"),
        b"selected generated name",
    )
    .unwrap();
    let req = request(
        root.path(),
        &["a.eml", "a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"],
        None,
    );
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(
        plan.records()[0].failure().unwrap().code(),
        ErrorCode::SourceDestinationAlias
    );
    assert!(plan.records()[1].failure().is_none());
    drop(plan);
    fs::remove_file(
        root.path()
            .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"),
    )
    .unwrap();
    fs::write(root.path().join("other.eml"), b"other selected source").unwrap();
    fs::hard_link(
        root.path().join("other.eml"),
        root.path()
            .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml"),
    )
    .unwrap();
    let req = request(root.path(), &["a.eml", "other.eml"], None);
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(
        plan.records()[0].failure().unwrap().code(),
        ErrorCode::SourceDestinationAlias
    );
    assert_eq!(
        fs::read(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        )
        .unwrap(),
        b"other selected source"
    );
    drop(plan);
    root.close().unwrap();
}

#[test]
fn truncated_human_stems_keep_distinct_full_route_tokens() {
    let root = scratch();
    fs::create_dir(root.path().join("out")).unwrap();
    let a = format!("{}A.eml", "x".repeat(250));
    let b = format!("{}B.eml", "x".repeat(250));
    fs::write(root.path().join(&a), b"a").unwrap();
    fs::write(root.path().join(&b), b"b").unwrap();
    let req = request(root.path(), &[&a, &b], Some("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert!(
        plan.records()
            .iter()
            .all(|record| record.failure().is_none())
    );
    assert_ne!(
        plan.records()[0].destination().unwrap().basename(),
        plan.records()[1].destination().unwrap().basename()
    );
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    drop(plan);
    root.close().unwrap();
}

#[test]
fn planning_stop_retains_all_sources_and_completed_discovery_without_planning_rows() {
    let root = scratch();
    fs::write(root.path().join("a.eml"), b"owned").unwrap();
    let req = request(root.path(), &["a.eml"], None);
    let sources = collect_sources(&req, &RunControl::default()).unwrap();
    let control = RunControl::default();
    control.request_stop();
    let plan = plan_destinations(sources, &control);
    assert_eq!(plan.state(), DestinationPlanState::Interrupted);
    assert!(plan.sources().collection_complete());
    assert_eq!(plan.sources().items().len(), 1);
    assert_eq!(plan.records().len(), 1);
    assert!(plan.records()[0].is_unplanned());
    assert!(plan.run_error().is_none());
    drop(plan);
    root.close().unwrap();
}

#[test]
fn blocked_collection_and_its_exact_native_failure_survive_planning_and_stop() {
    let root = scratch();
    let req = request(root.path(), &["missing.eml"], None);
    let sources = collect_sources(&req, &RunControl::default()).unwrap();
    let plan = plan_destinations(sources, &RunControl::default());
    assert_eq!(plan.state(), DestinationPlanState::CollectionBlocked);
    assert_eq!(
        plan.sources().run_error().unwrap().code(),
        ErrorCode::InputAdmissionFailed
    );
    assert!(!plan.sources().collection_complete());
    assert_eq!(plan.records().len(), 1);
    assert_eq!(
        plan.records()[0]
            .failure()
            .unwrap()
            .native_error()
            .unwrap()
            .raw_os_error(),
        Some(2)
    );
    drop(plan);
    let control = RunControl::default();
    control.request_stop();
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &control,
    );
    assert_eq!(plan.state(), DestinationPlanState::Interrupted);
    assert_eq!(
        plan.sources().run_error().unwrap().code(),
        ErrorCode::InputAdmissionFailed
    );
    assert_eq!(
        plan.records()[0]
            .failure()
            .unwrap()
            .native_error()
            .unwrap()
            .raw_os_error(),
        Some(2)
    );
    drop(plan);
    root.close().unwrap();
}

#[test]
fn selected_dot_uses_captured_working_label_without_creating_the_mirror() {
    let root = scratch();
    fs::create_dir(root.path().join("working-label")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::write(root.path().join("working-label/a.eml"), b"owned").unwrap();
    let mut req = request(&root.path().join("working-label"), &["."], None);
    req.output = OutputLocation::Directory(root.path().join("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert!(plan.records()[0].failure().is_none());
    assert_eq!(
        plan.display_path(0).unwrap().as_ref(),
        root.path()
            .join("out/working-label.eml-dir/a.onqh7ryqoe526hgwrt67tmyhxu.no-attachments.eml")
    );
    assert!(!root.path().join("out/working-label").exists());
    drop(plan);
    root.close().unwrap();
}

#[test]
fn nameless_nonroot_selection_keeps_selected_folder_label_after_normalization() {
    let root = scratch();
    fs::create_dir_all(root.path().join("selected/sub")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::write(root.path().join("selected/a.eml"), b"owned").unwrap();
    let req = request(root.path(), &["selected/sub/.."], Some("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert_eq!(plan.records().len(), 1);
    assert!(plan.records()[0].failure().is_none());
    assert_eq!(
        plan.display_path(0).unwrap().as_ref(),
        Path::new("out/selected-folder.eml-dir/a.e5ztshwilncrqadm4lqjqkxrgu.no-attachments.eml")
    );
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    drop(plan);
    root.close().unwrap();
}

#[test]
fn blocked_existing_mirror_does_not_mint_a_name_or_reservation_from_ancestor_limit() {
    let root = scratch();
    fs::create_dir(root.path().join("Inbox")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::write(root.path().join("Inbox/a.eml"), b"owned").unwrap();
    fs::write(
        root.path().join("out/Inbox.eml-dir"),
        b"owned incompatible parent",
    )
    .unwrap();
    let req = request(root.path(), &["Inbox"], Some("out"));
    let plan = plan_destinations(
        collect_sources(&req, &RunControl::default()).unwrap(),
        &RunControl::default(),
    );
    assert_eq!(plan.state(), DestinationPlanState::Complete);
    assert_eq!(plan.records().len(), 1);
    assert_eq!(
        plan.records()[0].failure().unwrap().code(),
        ErrorCode::DestinationExists
    );
    assert!(plan.records()[0].destination().is_none());
    assert!(plan.display_path(0).is_none());
    assert_eq!(
        fs::read(root.path().join("out/Inbox.eml-dir")).unwrap(),
        b"owned incompatible parent"
    );
    drop(plan);
    root.close().unwrap();
}

#[test]
fn actual_combined_path_budget_retains_all_source_positions_and_only_admitted_destinations() {
    let root = scratch();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let normalized = fs::canonicalize(root.path()).unwrap();
        assert!(
            normalized.as_os_str().encode_wide().count() <= 499,
            "quota fixture requires a normalized scratch prefix of at most 499 UTF-16 units; run this test from a shorter checkout path"
        );
    }
    let a = "a".repeat(255);
    let b = "b".repeat(255);
    let c = "c".repeat(255);
    let d = "d".repeat(255);
    let source = root.path().join(&a).join(&b);
    let output = root.path().join(&c).join(&d);
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&output).unwrap();
    let factor = if cfg!(windows) { 2 } else { 1 };
    let count = 4096 / factor;
    let prefix = format!("{}{a}/{b}/", "./".repeat(250));
    let out = format!("{}{c}/{d}", "./".repeat(250));
    let mut paths = Vec::new();
    for index in 0..count {
        let name = format!("{index:04}.eml");
        fs::write(source.join(&name), b"owned").unwrap();
        paths.push(PathBuf::from(format!("{prefix}{name}")));
    }
    assert_eq!(paths[0].as_os_str().len(), 1020);
    let req = RunRequest {
        paths,
        working_directory: root.path().to_owned(),
        output: OutputLocation::Directory(out.into()),
        mode: RunMode::DryRun,
    };
    assert!(req.validate_path_limits().is_ok());
    let sources = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(sources.items().len(), count);
    assert!(sources.collection_complete());
    let source_charge = count * 1020 * factor;
    assert_eq!(sources.source_path_bytes(), source_charge);
    let plan = plan_destinations(sources, &RunControl::default());
    assert_eq!(plan.state(), DestinationPlanState::InputLimitExceeded);
    assert!(plan.sources().collection_complete());
    assert_eq!(plan.sources().items().len(), count);
    assert_eq!(plan.records().len(), count);
    let destination_charge = 1062 * factor;
    let admitted = (8_388_608 - source_charge) / destination_charge;
    assert!(admitted < count);
    assert_eq!(
        plan.records()
            .iter()
            .filter(|row| row.destination().is_some())
            .count(),
        admitted
    );
    assert!(
        plan.records()[admitted..]
            .iter()
            .all(|row| row.is_unplanned())
    );
    assert_eq!(
        plan.reported_path_bytes(),
        source_charge + admitted * destination_charge
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    drop(plan);
    root.close().unwrap();
}

#[test]
fn ownership_names_and_execution_order_are_independent_of_argument_permutation() {
    let root = scratch();
    fs::create_dir_all(root.path().join("Inbox/deep")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    for file in ["Inbox/a.eml", "Inbox/deep/b.eml", "Inbox/deep/c.eml"] {
        fs::write(root.path().join(file), b"owned source").unwrap();
    }
    let selections = ["Inbox", "Inbox/deep", "Inbox/deep/b.eml"];
    let mut expected = None;
    for indices in [
        [0, 1, 2],
        [2, 1, 0],
        [1, 0, 2],
        [1, 2, 0],
        [0, 2, 1],
        [2, 0, 1],
    ] {
        let choices = indices.map(|index| selections[index]);
        let request = request(root.path(), &choices, Some("out"));
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let facts: Vec<_> = plan
            .records()
            .iter()
            .enumerate()
            .map(|(index, row)| {
                (
                    plan.sources().display_path(index).unwrap().into_owned(),
                    plan.display_path(index).unwrap().into_owned(),
                    plan.sources().items()[index].origin(),
                    row.failure().map(|error| error.code()),
                )
            })
            .collect();
        assert_eq!(facts.len(), 3);
        assert_eq!(
            facts
                .iter()
                .filter(|(_, _, origin, _)| *origin == eml_core::SourceOrigin::Explicit)
                .count(),
            1
        );
        if let Some(expected) = &expected {
            assert_eq!(&facts, expected);
        } else {
            expected = Some(facts);
        }
    }
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
}

#[test]
fn native_hardlinked_destinations_refuse_the_whole_identity_group_before_work() {
    let root = scratch();
    fs::create_dir(root.path().join("out")).unwrap();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join(name), b"selected source").unwrap();
    }
    let a = root
        .path()
        .join("out/a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    let b = root
        .path()
        .join("out/b.rsdsaffec2pcixjwzvef2ljmuu.no-attachments.eml");
    fs::write(&a, b"independent existing winner").unwrap();
    fs::hard_link(&a, &b).unwrap();
    for choices in [["a.eml", "b.eml"], ["b.eml", "a.eml"]] {
        let request = request(root.path(), &choices, Some("out"));
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        assert!(plan.records().iter().all(|row| {
            row.failure().is_some_and(|error| {
                error.code() == ErrorCode::DestinationCollision
                    && error.detail().contains("existing destination identity")
            })
        }));
    }
    assert_eq!(fs::read(&a).unwrap(), b"independent existing winner");
    assert_eq!(fs::read(&b).unwrap(), b"independent existing winner");
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 2);
}

#[test]
fn a_file_at_a_shared_mirror_refuses_every_dependent_route_without_writes() {
    let root = scratch();
    fs::create_dir(root.path().join("Inbox")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join("Inbox").join(name), b"selected source").unwrap();
    }
    let blocker = root.path().join("out/Inbox.eml-dir");
    fs::write(&blocker, b"independent prefix winner").unwrap();
    let request = request(root.path(), &["Inbox"], Some("out"));
    let control = RunControl::default();
    let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
    assert_eq!(plan.records().len(), 2);
    assert!(plan.records().iter().all(|row| {
        row.failure().is_some_and(|error| {
            error.code() == ErrorCode::DestinationCollision
                && error.detail().contains("file-versus-mirror prefix")
        })
    }));
    assert_eq!(fs::read(&blocker).unwrap(), b"independent prefix winner");
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 1);
}
