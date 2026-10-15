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
    use std::{ffi::OsStr, io, os::unix::process::CommandExt, path::Path, process::Command};
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
                        matches!(held.observe_child(OsStr::new("stage"), false), Err(ref e) if e.kind() == io::ErrorKind::NotFound),
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
            "rename" => {
                std::fs::rename(parent.join("stage"), parent.join("moved"))?;
                println!(
                    "Negative control: same actor can rename victim-owned entry in shared nonsticky parent"
                );
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
            parent.observe_child(OsStr::new(name), false)?.identity == expected,
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
    pub fn main() -> Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("--noop") {
            return Ok(());
        }
        if args.first().map(String::as_str) == Some("--child") {
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
            .observe_child(OsStr::new("probe"), false)?
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
            let stage = fixture
                .directory()
                .observe_child(OsStr::new("stage"), false)?
                .identity;
            let actor_control = if mode == 0o1777 { path } else { control_path };
            run_child(
                &executable,
                actor,
                actor,
                "deny-rename",
                path,
                actor_control,
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
        fs::fchown(
            shared.directory(),
            Some(Uid::from_raw(victim)),
            Some(Gid::from_raw(victim_gid)),
        )?;
        fs::fchmod(shared.directory(), Mode::from_raw_mode(0o777))?;
        run_child(
            &executable,
            actor,
            actor,
            "rename",
            shared.directory().requested_path(),
            shared.directory().requested_path(),
        )?;
        fs::fchown(shared.directory(), Some(Uid::ROOT), Some(Gid::ROOT))?;
        fs::fchmod(shared.directory(), Mode::from_raw_mode(0o700))?;
        remove_entry(shared.directory(), "moved", stage_id, true)?;
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
