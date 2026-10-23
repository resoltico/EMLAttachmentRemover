//! Ordered session cleanup under live membership and unreaped group-pin authority.
use super::*;

impl Session {
    pub(super) fn cleanup(
        &mut self,
        ordinary_exit: bool,
    ) -> (
        bool,
        usize,
        Duration,
        Option<String>,
        Vec<serde_json::Value>,
    ) {
        let total_began = Instant::now();
        let cooperative_error = if ordinary_exit {
            None
        } else {
            self.cooperate().err().map(|error| error.to_string())
        };
        let began = Instant::now();
        let mut detail = cooperative_error;
        let (unexpected, observations) =
            self.discover_quiescence(ordinary_exit, began, &mut detail);
        self.terminate_groups(&mut detail);
        self.reap_worker(began, &mut detail);
        self.confirm_termination(began, &mut detail);
        self.reap_pins(began, &mut detail);
        if self.worker.is_some() && self.raw_status.is_none() {
            detail.get_or_insert_with(|| "Worker could not be reaped within cleanup grace".into());
        }
        self.cleaned = detail.is_none();
        (
            self.cleaned,
            unexpected.len(),
            total_began.elapsed(),
            detail,
            observations.into_values().collect(),
        )
    }
    fn discover_quiescence(
        &mut self,
        ordinary_exit: bool,
        began: Instant,
        detail: &mut Option<String>,
    ) -> (BTreeSet<i32>, BTreeMap<i32, serde_json::Value>) {
        let mut unexpected = BTreeSet::new();
        let mut observations = BTreeMap::new();
        let mut stable = 0;
        let discovery_until = began + CLEANUP_GRACE / 2;
        // Freeze the known root first, including cargo-mutants' spawning/reaping threads.
        for pin in self.pins.values() {
            let _ = kill_process_group(pin.id, Signal::STOP);
        }
        while began.elapsed() < CLEANUP_GRACE / 2 {
            let scan = inspect(
                &self.inspector,
                self.id,
                InspectionScope::Session,
                discovery_until,
            );
            let (rows, unstable) = match scan {
                Ok(scan) => scan,
                Err(error) => {
                    detail.get_or_insert_with(|| format!("Quiescence inspection failed: {error}"));
                    break;
                }
            };
            let (added, running) = self.freeze_observed_rows(
                rows,
                ordinary_exit,
                discovery_until,
                &mut unexpected,
                &mut observations,
                detail,
            );
            if detail.is_some() {
                break;
            }
            if !added && !running && !unstable {
                stable += 1;
            } else {
                stable = 0;
            }
            if stable >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if stable < 2 && detail.is_none() {
            detail.get_or_insert_with(|| {
                "Session quiescence remained unconfirmed within cleanup grace".into()
            });
        }
        (unexpected, observations)
    }
    fn freeze_observed_rows(
        &mut self,
        rows: Vec<Row>,
        ordinary_exit: bool,
        discovery_until: Instant,
        unexpected: &mut BTreeSet<i32>,
        observations: &mut BTreeMap<i32, serde_json::Value>,
        detail: &mut Option<String>,
    ) -> (bool, bool) {
        let pins = self
            .pins
            .values()
            .map(|pin| pin.child.id() as i32)
            .collect::<BTreeSet<_>>();
        let mut added = false;
        let mut running = false;
        for row in rows {
            if Instant::now() >= discovery_until {
                detail.get_or_insert_with(|| "Cleanup discovery budget exhausted".into());
                break;
            }
            let pid = number(row.pid);
            if row.pid == self.id || pins.contains(&pid) {
                continue;
            }
            if row.group == number(self.id) {
                detail.get_or_insert_with(|| {
                    "Unexpected worker joined the supervisor's protected group".into()
                });
                continue;
            }
            if ordinary_exit && !row.state.starts_with('Z') {
                unexpected.insert(pid);
                if observations.len() < 64 {
                    observations.entry(pid).or_insert_with(||serde_json::json!({"pid":pid,"group":row.group,"state":row.state,"command":row.command,"scope":"observed owned-session member; name is diagnostic only"}));
                }
            }
            if !self.pins.contains_key(&row.group) {
                let Some(group) = Pid::from_raw(row.group) else {
                    detail.get_or_insert_with(|| "Invalid inspected group ID".into());
                    continue;
                };
                match pin(&self.helper, self.id, Some(group), discovery_until) {
                    Ok(held) => {
                        self.pins.insert(row.group, held);
                        added = true;
                    }
                    Err(_) => {
                        running = true;
                        continue;
                    }
                }
            }
            if let Some(held) = self.pins.get(&row.group) {
                let _ = kill_process_group(held.id, Signal::STOP);
            }
            if !row.state.starts_with('T') && !row.state.starts_with('Z') {
                running = true;
            }
        }
        (added, running)
    }
    fn terminate_groups(&self, detail: &mut Option<String>) {
        // Every signal below is authorized by a retained, unreaped pin. No pin is
        // polled/reaped until its last group signal, even if already a zombie.
        for held in self.pins.values() {
            if let Err(error) = kill_process_group(held.id, Signal::KILL)
                && error != rustix::io::Errno::SRCH
            {
                detail.get_or_insert_with(|| "Owned group termination failed".into());
            }
        }
    }
    fn reap_worker(&mut self, began: Instant, detail: &mut Option<String>) {
        if self.raw_status.is_none()
            && let Some(worker) = &mut self.worker
        {
            while began.elapsed() < CLEANUP_GRACE {
                match worker.try_wait() {
                    Ok(Some(status)) => {
                        self.raw_status = Some(status.into_raw());
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(error) => {
                        detail.get_or_insert_with(|| error.to_string());
                        break;
                    }
                }
            }
        }
    }
    fn confirm_termination(&self, began: Instant, detail: &mut Option<String>) {
        let cleanup_until = began + CLEANUP_GRACE;
        let mut clean_observations = 0;
        while began.elapsed() < CLEANUP_GRACE {
            match inspect(
                &self.inspector,
                self.id,
                InspectionScope::Session,
                cleanup_until,
            ) {
                Ok((rows, unstable)) => {
                    if !unstable
                        && !rows
                            .iter()
                            .any(|row| row.pid != self.id && !row.state.starts_with('Z'))
                    {
                        clean_observations += 1;
                    } else {
                        clean_observations = 0;
                    }
                    if clean_observations >= 2 {
                        break;
                    }
                }
                Err(error) => {
                    detail.get_or_insert_with(|| {
                        format!("Post-termination inspection failed: {error}")
                    });
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if clean_observations < 2 {
            detail.get_or_insert_with(|| {
                "Post-termination live-member observation is unconfirmed".into()
            });
        }
    }
    fn reap_pins(&mut self, began: Instant, detail: &mut Option<String>) {
        // Pins can now be reaped; no later group signal will use their numbers.
        for held in self.pins.values_mut() {
            let mut reaped = false;
            while began.elapsed() < CLEANUP_GRACE {
                match held.child.try_wait() {
                    Ok(Some(_)) => {
                        reaped = true;
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
            if !reaped {
                detail.get_or_insert_with(|| {
                    "Owned pin could not be reaped within cleanup grace".into()
                });
            }
        }
        self.pins.clear();
    }
}
