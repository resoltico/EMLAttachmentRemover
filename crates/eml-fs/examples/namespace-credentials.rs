//! Native credential-boundary probe. Only explicit invocation under an ephemeral
//! runner's sudo creates resources; normal unit tests never request credentials.
#![forbid(unsafe_code)]
#[cfg(unix)]
mod probe {
    use eml_fs::{BoundDirectory, CleanupState, CreatedDirectory, Identity};
    use rustix::{
        fs::{self, AtFlags, Mode, OFlags},
        process::{self, Gid, Uid},
    };
    use std::{
        ffi::OsStr,
        io,
        os::unix::{fs::DirBuilderExt, process::CommandExt},
        path::Path,
        process::Command,
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    fn require(value: bool, reason: &str) -> Result<()> {
        if value { Ok(()) } else { Err(reason.into()) }
    }
    fn ids(uid: u32, gid: u32) -> Result<()> {
        println!(
            "Credential observation: requested uid={uid} gid={gid}; actual ruid={} euid={} rgid={} egid={} groups={:?}",
            process::getuid().as_raw(),
            process::geteuid().as_raw(),
            process::getgid().as_raw(),
            process::getegid().as_raw(),
            process::getgroups()?
        );
        require(
            process::getuid().as_raw() == uid && process::geteuid().as_raw() == uid,
            "Wrong actual real/effective UID",
        )?;
        require(
            process::getgid().as_raw() == gid && process::getegid().as_raw() == gid,
            "Wrong actual real/effective GID",
        )?;
        let groups = process::getgroups()?;
        require(
            groups.iter().all(|group| group.as_raw() == gid),
            "Unexpected supplementary group authority",
        )?;
        let regain = Command::new(std::env::current_exe()?)
            .arg("--noop")
            .uid(0)
            .status();
        require(
            matches!(regain, Err(ref error) if error.raw_os_error() == Some(libc::EPERM)),
            "Dropped child could regain root or had an unexpected spawn failure",
        )?;
        println!(
            "Observed real/effective uid={uid} gid={gid} supplementary={groups:?}; root regain EPERM"
        );
        Ok(())
    }
    fn positive(parent: &Path, uid: u32) -> Result<()> {
        let before = parent.join(format!("actor-{uid}"));
        let after = parent.join(format!("actor-{uid}-moved"));
        std::fs::create_dir(&before)?;
        std::fs::rename(&before, &after)?;
        std::fs::remove_dir(&after)?;
        println!("Positive control: same dropped credentials create/rename/remove their own entry");
        Ok(())
    }
    fn child(args: &[String]) -> Result<()> {
        require(args.len() == 5, "Malformed credential child invocation")?;
        let uid = args[0].parse()?;
        let gid = args[1].parse()?;
        ids(uid, gid)?;
        let action = args[2].as_str();
        let parent = Path::new(&args[3]);
        let control = Path::new(&args[4]);
        positive(control, uid)?;
        match action {
            "positive" => {}
            "deny-create" => {
                let error = std::fs::create_dir(parent.join("blocked"))
                    .err()
                    .ok_or("Masked actor created an entry")?;
                require(
                    matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)),
                    "Creation denial was unrelated to access control",
                )?;
                println!("Named-ACL zero-mask actor create refused: {error}");
            }
            "admit" | "refuse" => {
                let held = BoundDirectory::open(parent)?;
                let result = held.create_private_child(OsStr::new("stage"));
                if action == "admit" {
                    let created = result?;
                    require(
                        fs::fstat(created.directory())?.st_mode & 0o777 == 0o700,
                        "Victim stage was not0700",
                    )?;
                    created.persist();
                } else {
                    let error = result
                        .err()
                        .ok_or("Unsafe namespace unexpectedly admitted")?;
                    require(
                        error.error.kind() == io::ErrorKind::Unsupported,
                        "Refusal was unrelated to namespace admission",
                    )?;
                    require(
                        matches!(held.observe_child(OsStr::new("stage")), Err(ref e) if e.kind() == io::ErrorKind::NotFound),
                        "Refused operation created an entry",
                    )?;
                }
            }
            "deny-rename" => {
                let error = std::fs::rename(parent.join("stage"), parent.join("moved"))
                    .err()
                    .ok_or("Other UID renamed victim stage")?;
                require(
                    matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)),
                    "Rename denial was not an access-control result",
                )?;
                println!("Other-UID victim rename refused: {error}");
            }
            "deny-empty-removal" => {
                let error = std::fs::remove_dir(parent.join("stage"))
                    .err()
                    .ok_or("Other UID removed victim empty stage")?;
                require(
                    matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)),
                    "Empty removal denial was not an access-control result",
                )?;
                println!("Other-UID empty victim0700 removal refused: {error}");
            }
            "replace-empty" => {
                std::fs::remove_dir(parent.join("stage"))?;
                println!("Native rmdir completed on the original empty victim0700 entry");
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(parent.join("stage"))?;
                let held = BoundDirectory::open(parent)?.open_child(OsStr::new("stage"))?;
                let Identity::Posix { device, inode } = held.identity() else {
                    return Err("Unexpected identity kind".into());
                };
                println!("owned-replacement:{device}:{inode}");
            }
            _ => return Err("Unknown probe action".into()),
        }
        Ok(())
    }
    fn run_child(
        executable: &Path,
        uid: u32,
        gid: u32,
        action: &str,
        fixture: &Path,
        control: &Path,
    ) -> Result<()> {
        println!("Credential action {action}: uid={uid} gid={gid}");
        let status = Command::new(executable)
            .arg("--child")
            .arg(uid.to_string())
            .arg(gid.to_string())
            .arg(action)
            .arg(fixture)
            .arg(control)
            .uid(uid)
            .gid(gid)
            .status()?;
        require(
            status.success(),
            "Dropped-credential child failed; this is not passing protection evidence",
        )
    }
    fn run_replacement(executable: &Path, actor: u32, fixture: &Path) -> Result<Identity> {
        let output = Command::new(executable)
            .arg("--child")
            .arg(actor.to_string())
            .arg(actor.to_string())
            .arg("replace-empty")
            .arg(fixture)
            .arg(fixture)
            .uid(actor)
            .gid(actor)
            .output()?;
        use std::io::Write;
        io::stdout().write_all(&output.stdout)?;
        io::stderr().write_all(&output.stderr)?;
        require(
            output.status.success(),
            "Actual empty-directory removal/replacement control failed",
        )?;
        let text = std::str::from_utf8(&output.stdout)?;
        let records: Vec<_> = text
            .lines()
            .filter_map(|line| line.strip_prefix("owned-replacement:"))
            .collect();
        require(
            records.len() == 1,
            "Missing or repeated actor-owned replacement identity",
        )?;
        let (device, inode) = records[0]
            .split_once(':')
            .ok_or("Malformed replacement identity")?;
        Ok(Identity::Posix {
            device: device.parse()?,
            inode: inode.parse()?,
        })
    }
    fn root_child(
        parent: &BoundDirectory,
        name: &str,
        owner: u32,
        gid: u32,
        mode: Mode,
    ) -> Result<CreatedDirectory> {
        let created = parent.create_private_child(OsStr::new(name))?;
        fs::fchown(
            created.directory(),
            Some(Uid::from_raw(owner)),
            Some(Gid::from_raw(gid)),
        )?;
        fs::fchmod(created.directory(), mode)?;
        Ok(created)
    }
    fn remove_entry(
        parent: &BoundDirectory,
        name: &str,
        expected: Identity,
        directory: bool,
    ) -> Result<()> {
        parent.admit_namespace()?;
        require(
            parent.observe_child(OsStr::new(name))?.identity == expected,
            "Owned cleanup identity changed; resource retained",
        )?;
        fs::unlinkat(
            parent,
            name,
            if directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    fn acl_masks(
        outer: &BoundDirectory,
        executable: &Path,
        victim: u32,
        victim_gid: u32,
        actor: u32,
        control: &Path,
    ) -> Result<()> {
        let fixture = root_child(
            outer,
            "acl-mask",
            victim,
            victim_gid,
            Mode::from_raw_mode(0o700),
        )?;
        let path = fixture.directory().requested_path();
        let set_mask = |mask: &str| -> Result<()> {
            require(
                Command::new("setfacl")
                    .arg("-m")
                    .arg(format!("u:{actor}:rwx,m::{mask}"))
                    .arg(path)
                    .status()?
                    .success(),
                "Native setfacl failed",
            )?;
            require(
                Command::new("getfacl")
                    .args(["--numeric", "--absolute-names"])
                    .arg(path)
                    .status()?
                    .success(),
                "Native getfacl failed",
            )
        };
        set_mask("rwx")?;
        // The named user really has write authority; fail-closed mode-mask
        // admission must refuse before mkdir, rather than trust base group bits.
        run_child(executable, actor, actor, "positive", path, path)?;
        run_child(executable, victim, victim_gid, "refuse", path, control)?;
        set_mask("---")?;
        run_child(executable, victim, victim_gid, "admit", path, control)?;
        let stage = fixture
            .directory()
            .observe_child(OsStr::new("stage"))?
            .identity;
        run_child(executable, actor, actor, "deny-create", path, control)?;
        fs::fchown(fixture.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
        fs::fchmod(fixture.directory(), Mode::from_raw_mode(0o700))?;
        remove_entry(fixture.directory(), "stage", stage, true)?;
        require(
            fixture.remove_empty().state == CleanupState::Complete,
            "ACL-mask fixture cleanup unconfirmed",
        )?;
        println!(
            "PASS: actual named-ACL write grant, pre-mkdir refusal, zero-mask product admission and other-UID creation denial"
        );
        Ok(())
    }
    pub fn main() -> Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("--noop") {
            return Ok(());
        }
        if args.first().map(String::as_str) == Some("--child") {
            #[cfg(target_os = "macos")]
            {
                require(args.len() == 6, "Malformed credential launcher")?;
                let uid: u32 = args[1].parse()?;
                let gid: u32 = args[2].parse()?;
                require(
                    uid != 0
                        && gid != 0
                        && process::getuid().as_raw() == uid
                        && process::geteuid().as_raw() == uid
                        && process::getgid().as_raw() == gid,
                    "Darwin credential launcher has wrong UID or real GID",
                )?;
                let groups = process::getgroups()?;
                require(
                    process::getegid().as_raw() == 0
                        && groups.len() == 1
                        && groups[0].as_raw() == 0,
                    "Darwin launcher did not match the reviewed zero-group normalization state",
                )?;
                println!(
                    "Darwin pre-normalization: ruid={} euid={} rgid={} egid={} groups={:?}",
                    process::getuid().as_raw(),
                    process::geteuid().as_raw(),
                    process::getgid().as_raw(),
                    process::getegid().as_raw(),
                    process::getgroups()?
                );
                // Rust1.99 clears groups after setgid when uid is requested.
                // Native Darwin evidence shows that empty group list resets the
                // effective primary group to0 while the real group stays correct.
                // A second safe exec requests only our permitted real GID, so it
                // does not clear groups again. No fixture operation occurs until
                // the final child strictly verifies all credentials and no regain.
                let error = Command::new(std::env::current_exe()?)
                    .arg("--verified-child")
                    .args(&args[1..])
                    .gid(gid)
                    .exec();
                return Err(error.into());
            }
            #[cfg(not(target_os = "macos"))]
            return child(&args[1..]);
        }
        if args.first().map(String::as_str) == Some("--verified-child") {
            return child(&args[1..]);
        }
        require(
            args.len() == 2,
            "Usage: sudo -n namespace-credentials VICTIM_UID VICTIM_GID",
        )?;
        require(
            process::getuid().is_root() && process::geteuid().is_root(),
            "Native credential gate requires available noninteractive root; otherwise UNRUN",
        )?;
        let victim: u32 = args[0].parse()?;
        let victim_gid: u32 = args[1].parse()?;
        require(
            victim != 0 && victim_gid != 0,
            "Victim must be a nonroot runner account",
        )?;
        #[cfg(target_os = "linux")]
        let overflow: u32 = std::fs::read_to_string("/proc/sys/kernel/overflowuid")?
            .trim()
            .parse()?;
        #[cfg(not(target_os = "linux"))]
        let overflow = u32::MAX;
        let mut identities =
            (65_528..65_534).filter(|uid| *uid != victim && *uid != victim_gid && *uid != overflow);
        let actor = identities.next().ok_or("No distinct actor identity")?;
        let foreign = identities
            .next()
            .ok_or("No distinct foreign-owner identity")?;
        #[cfg(target_os = "linux")]
        for tool in ["setfacl", "getfacl"] {
            require(
                Command::new(tool).arg("--version").status()?.success(),
                "Install the native POSIX ACL test tools before running this credential gate",
            )?;
        }
        let temporary = BoundDirectory::open(Path::new("/tmp"))?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)?;
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let outer =
            temporary.create_private_child(OsStr::new(&format!("eml-namespace-{suffix}")))?;
        // No automatic recursive/root pathname cleanup. On failure, retain the
        // exact owned fixture and print its address for investigation.
        println!(
            "Owned native credential fixture: {}",
            outer.directory().requested_path().display()
        );
        fs::fchmod(outer.directory(), Mode::from_raw_mode(0o755))?;
        let executable = outer.directory().requested_path().join("probe");
        let fd = fs::openat(
            outer.directory(),
            "probe",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o700),
        )?;
        let mut copy = std::fs::File::from(fd);
        io::copy(
            &mut std::fs::File::open(std::env::current_exe()?)?,
            &mut copy,
        )?;
        fs::fchmod(&copy, Mode::from_raw_mode(0o755))?;
        let executable_id = outer
            .directory()
            .observe_child(OsStr::new("probe"))?
            .identity;
        drop(copy);
        let control = root_child(
            outer.directory(),
            "control",
            0,
            0,
            Mode::from_raw_mode(0o1777),
        )?;
        let control_path = control.directory().requested_path();
        for (name, owner, group, mode) in [
            ("private", victim, victim_gid, 0o700),
            ("sticky", victim, victim_gid, 0o1777),
            ("trusted-root-sticky", 0, 0, 0o1777),
        ] {
            let fixture = root_child(
                outer.directory(),
                name,
                owner,
                group,
                Mode::from_raw_mode(mode),
            )?;
            let path = fixture.directory().requested_path();
            run_child(&executable, victim, victim_gid, "admit", path, control_path)?;
            let stage_handle = fixture.directory().open_child(OsStr::new("stage"))?;
            let stage = stage_handle.identity();
            let actor_control = if mode == 0o1777 { path } else { control_path };
            run_child(
                &executable,
                actor,
                actor,
                "deny-rename",
                path,
                actor_control,
            )?;
            run_child(
                &executable,
                actor,
                actor,
                "deny-empty-removal",
                path,
                actor_control,
            )?;
            require(
                fixture
                    .directory()
                    .observe_child(OsStr::new("stage"))?
                    .identity
                    == stage
                    && fs::fstat(&stage_handle)?.st_nlink != 0,
                "Protected empty stage changed identity or was removed",
            )?;
            // Reclaim only this freshly owned fixture through its retained FD;
            // protect cleanup before any identity check plus namespace removal.
            fs::fchown(fixture.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
            fs::fchmod(fixture.directory(), Mode::from_raw_mode(0o700))?;
            remove_entry(fixture.directory(), "stage", stage, true)?;
            require(
                fixture.remove_empty().state == CleanupState::Complete,
                "Fixture cleanup unconfirmed",
            )?;
            println!("PASS real different-UID namespace mode {mode:o}");
        }
        let shared = root_child(
            outer.directory(),
            "shared",
            victim,
            victim_gid,
            Mode::from_raw_mode(0o777),
        )?;
        run_child(
            &executable,
            victim,
            victim_gid,
            "refuse",
            shared.directory().requested_path(),
            control_path,
        )?;
        // Owned negative control establishes actual attack authority in the
        // refused model, without weakening product admission to create it.
        // Protect the freshly owned parent before privileged creation/binding;
        // expose the unsafe model only after the created identity is known.
        fs::fchown(shared.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
        fs::fchmod(shared.directory(), Mode::from_raw_mode(0o700))?;
        let held = shared
            .directory()
            .create_private_child(OsStr::new("stage"))?
            .persist();
        fs::fchown(
            &held,
            Some(Uid::from_raw(victim)),
            Some(Gid::from_raw(victim_gid)),
        )?;
        let stage_id = held.identity();
        let before_links = fs::fstat(&held)?.st_nlink;
        fs::fchown(
            shared.directory(),
            Some(Uid::from_raw(victim)),
            Some(Gid::from_raw(victim_gid)),
        )?;
        fs::fchmod(shared.directory(), Mode::from_raw_mode(0o777))?;
        // This parent was refused before product mkdir. Do not depend on the
        // additional rename denial observed on older Darwin versions: macOS26
        // permits that rename. The actual other-UID removal/replacement below
        // proves why stage0700 alone is insufficient on this refused model.
        let replacement = run_replacement(&executable, actor, shared.directory().requested_path())?;
        fs::fchown(shared.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
        fs::fchmod(shared.directory(), Mode::from_raw_mode(0o700))?;
        let original = fs::fstat(&held)?;
        #[allow(clippy::unnecessary_cast)]
        // Independent native oracle: Darwin dev_t signed32, Linux unsigned64.
        let retained_id = Identity::Posix {
            device: original.st_dev as u64,
            inode: original.st_ino,
        };
        require(
            retained_id == stage_id,
            "Retained original directory handle changed identity",
        )?;
        println!(
            "Retained original directory link counts before={before_links} after={}; native rmdir receipt and distinct replacement qualify the controlled name removal",
            original.st_nlink
        );
        #[cfg(target_os = "linux")]
        require(
            original.st_nlink == 0,
            "Linux removed directory link-count observation changed",
        )?;
        require(
            replacement != stage_id,
            "Replacement reused the still-held original identity",
        )?;
        let actual = fs::statat(shared.directory(), "stage", AtFlags::SYMLINK_NOFOLLOW)?;
        require(
            actual.st_uid == actor && actual.st_mode & 0o777 == 0o700,
            "Actor replacement ownership/mode mismatch",
        )?;
        remove_entry(shared.directory(), "stage", replacement, true)?;
        println!(
            "PASS: native other-UID rmdir and replacement; retained original and distinct actor identity verified"
        );
        require(
            shared.remove_empty().state == CleanupState::Complete,
            "Shared fixture cleanup unconfirmed",
        )?;
        for mode in [0o555, 0o1777] {
            let fixture = root_child(
                outer.directory(),
                "foreign",
                foreign,
                foreign,
                Mode::from_raw_mode(mode),
            )?;
            run_child(
                &executable,
                victim,
                victim_gid,
                "refuse",
                fixture.directory().requested_path(),
                control_path,
            )?;
            fs::fchown(fixture.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
            fs::fchmod(fixture.directory(), Mode::from_raw_mode(0o700))?;
            require(
                fixture.remove_empty().state == CleanupState::Complete,
                "Foreign fixture cleanup unconfirmed",
            )?;
        }
        #[cfg(target_os = "linux")]
        acl_masks(
            outer.directory(),
            &executable,
            victim,
            victim_gid,
            actor,
            control_path,
        )?;
        require(
            control.remove_empty().state == CleanupState::Complete,
            "Control cleanup unconfirmed",
        )?;
        require(
            outer.directory().check_requested_address()?,
            "Owned fixture requested address changed",
        )?;
        remove_entry(outer.directory(), "probe", executable_id, false)?;
        require(
            outer.remove_empty().state == CleanupState::Complete,
            "Outer cleanup unconfirmed",
        )?;
        println!(
            "PASS: actual dropped credentials, own-entry positive controls, private/sticky victim protection, shared negative control, foreign-owner refusals; owned resources removed"
        );
        Ok(())
    }
}
#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::main()
}
#[cfg(not(unix))]
fn main() {
    eprintln!("This native POSIX credential probe is unavailable on this platform");
    std::process::exit(1);
}
