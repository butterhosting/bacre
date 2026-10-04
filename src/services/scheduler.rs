//! A due backup waits in a set of (backend, service) pairs, so however long the runner stays
//! busy it waits at most once. At startup, a backend whose newest snapshot predates its last
//! due moment gets one catch-up run; one without snapshots, or that could not be listed, waits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Local, Utc};

use super::archive_service::ArchiveService;
use crate::cron::Cron;
use crate::jobs::job_service::JobService;
use crate::models::archives::{Backend, BackendState, Schedule, Service, iso};
use crate::models::atlas::Entry;
use crate::models::jobs::{BackupRequest, BtrfsTarget, Mode, Request, ResticTarget, Trigger};

const CHECK: Duration = Duration::from_secs(20);

pub trait Atlas: Send + Sync {
    fn entries(&self) -> Vec<Entry>;
    fn services(&self) -> Vec<Service>;
}

pub trait Runner: Send + Sync {
    fn idle(&self) -> bool;
    fn start(&self, request: Request);
}

impl Atlas for ArchiveService {
    fn entries(&self) -> Vec<Entry> {
        ArchiveService::entries(self)
    }

    fn services(&self) -> Vec<Service> {
        ArchiveService::services(self)
    }
}

impl Runner for Arc<JobService> {
    fn idle(&self) -> bool {
        JobService::idle(self)
    }

    fn start(&self, request: Request) {
        // idle was checked a moment ago; should a manual job have slipped in, the pairs wait for the next slot
        let _ = JobService::start(self, request, Trigger::Schedule);
    }
}

pub struct Scheduler {
    atlas: Arc<dyn Atlas>,
    runner: Box<dyn Runner>,
    now: Box<dyn Fn() -> DateTime<Local> + Send + Sync>,
    state: Mutex<State>,
}

struct State {
    waiting: BTreeMap<Backend, BTreeSet<String>>,
    last_checked: DateTime<Local>,
    stopped: bool,
}

struct Scheduled {
    service: String,
    backend: Backend,
    cron: Cron,
}

impl Scheduler {
    pub fn new(atlas: Arc<dyn Atlas>, runner: Box<dyn Runner>) -> Arc<Self> {
        Self::with_clock(atlas, runner, Box::new(Local::now))
    }

    pub fn with_clock(
        atlas: Arc<dyn Atlas>,
        runner: Box<dyn Runner>,
        now: Box<dyn Fn() -> DateTime<Local> + Send + Sync>,
    ) -> Arc<Self> {
        let last_checked = now();
        Arc::new(Self {
            atlas,
            runner,
            now,
            state: Mutex::new(State {
                waiting: BTreeMap::new(),
                last_checked,
                stopped: false,
            }),
        })
    }

    pub fn start(self: &Arc<Self>) {
        self.state.lock().unwrap().last_checked = (self.now)();
        self.catch_up();
        let scheduler = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CHECK).await;
                match scheduler.upgrade() {
                    Some(scheduler) if !scheduler.state.lock().unwrap().stopped => {
                        scheduler.check()
                    }
                    _ => break,
                }
            }
        });
        self.drain();
    }

    pub fn stop(&self) {
        self.state.lock().unwrap().stopped = true;
    }

    pub fn forget_waiting(&self) {
        self.state.lock().unwrap().waiting.clear();
    }

    pub fn check(&self) {
        let now = (self.now)();
        let last_checked = std::mem::replace(&mut self.state.lock().unwrap().last_checked, now);
        for scheduled in self.scheduled() {
            if let Some(due) = scheduled
                .cron
                .previous(&now)
                .filter(|due| *due > last_checked)
            {
                self.enqueue(&scheduled, &format!("is due ({})", due.format("%H:%M")));
            }
        }
        self.drain();
    }

    pub fn list(&self) -> Vec<Schedule> {
        let now = (self.now)();
        let state = self.state.lock().unwrap();
        self.scheduled()
            .into_iter()
            .map(|scheduled| Schedule {
                next: scheduled
                    .cron
                    .next(&now)
                    .map(|next| iso(next.with_timezone(&Utc))),
                waiting: state
                    .waiting
                    .get(&scheduled.backend)
                    .is_some_and(|services| services.contains(&scheduled.service)),
                cron: scheduled.cron.expression().to_string(),
                service: scheduled.service,
                backend: scheduled.backend,
            })
            .collect()
    }

    pub fn drain(&self) {
        let request = {
            let mut state = self.state.lock().unwrap();
            if state.stopped || !self.runner.idle() {
                return;
            }
            let entries = self.atlas.entries();
            Backend::ALL.into_iter().find_map(|backend| {
                // a service whose bacre.yaml dropped the backend in the meantime is not backed up
                let services: Vec<String> =
                    std::mem::take(state.waiting.entry(backend).or_default())
                        .into_iter()
                        .filter(|service| {
                            entries.iter().any(|entry| {
                                entry.config.service == *service && entry.config.configures(backend)
                            })
                        })
                        .collect();
                (!services.is_empty()).then(|| request(backend, services))
            })
        };
        if let Some(request) = request {
            self.runner.start(request);
        }
    }

    fn catch_up(&self) {
        let now = (self.now)();
        let services = self.atlas.services();
        for scheduled in self.scheduled() {
            let Some(listed) = services
                .iter()
                .find(|service| service.name == scheduled.service)
            else {
                continue;
            };
            // no history, or no way of knowing: wait for the next regular slot
            if listed
                .backends
                .get(&scheduled.backend)
                .map(|status| status.state)
                != Some(BackendState::Ok)
            {
                continue;
            }
            let Some(newest) = listed
                .snapshots
                .iter()
                .find(|snapshot| snapshot.backend == scheduled.backend)
            else {
                continue;
            };
            let Ok(newest) = DateTime::parse_from_rfc3339(&newest.time) else {
                continue;
            };
            if let Some(due) = scheduled.cron.previous(&now).filter(|due| newest < *due) {
                self.enqueue(
                    &scheduled,
                    &format!("missed its run of {}", due.format("%H:%M")),
                );
            }
        }
    }

    fn enqueue(&self, scheduled: &Scheduled, why: &str) {
        let added = self
            .state
            .lock()
            .unwrap()
            .waiting
            .entry(scheduled.backend)
            .or_default()
            .insert(scheduled.service.clone());
        let already = if added {
            ""
        } else {
            ", and was already waiting"
        };
        println!(
            "==> Schedule: {} · {} {why}{already}",
            scheduled.backend, scheduled.service
        );
    }

    fn scheduled(&self) -> Vec<Scheduled> {
        let mut scheduled = Vec::new();
        for entry in self.atlas.entries() {
            for backend in Backend::ALL {
                if let Some(cron) = entry
                    .config
                    .schedule(backend)
                    .and_then(|expression| Cron::parse(expression).ok())
                {
                    scheduled.push(Scheduled {
                        service: entry.config.service.clone(),
                        backend,
                        cron,
                    });
                }
            }
        }
        scheduled
    }
}

fn request(backend: Backend, mut services: Vec<String>) -> Request {
    services.sort();
    Request::Backup(match backend {
        // hot: a schedule never stops a service
        Backend::Btrfs => BackupRequest::Btrfs {
            targets: services
                .into_iter()
                .map(|service| BtrfsTarget {
                    service,
                    mode: Mode::Hot,
                })
                .collect(),
        },
        Backend::Restic => BackupRequest::Restic {
            targets: services
                .into_iter()
                .map(|service| ResticTarget { service })
                .collect(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::models::archives::{
        BackendInfo, BackendStatus, HooksInfo, ResticRetentionInfo, Snapshot, SnapshotDetails,
    };
    use chrono::NaiveDateTime;

    use crate::cron;
    use crate::models::atlas;

    #[derive(Default, Clone)]
    struct Plan {
        btrfs: Option<&'static str>,
        restic: Option<&'static str>,
        newest_restic: Option<&'static str>,
        restic_state: Option<BackendState>,
    }

    struct Fakes {
        entries: Vec<Entry>,
        services: Vec<Service>,
        idle: AtomicBool,
        started: Mutex<Vec<Request>>,
        now: Mutex<DateTime<Local>>,
    }

    impl Atlas for Fakes {
        fn entries(&self) -> Vec<Entry> {
            self.entries.clone()
        }

        fn services(&self) -> Vec<Service> {
            self.services.clone()
        }
    }

    struct FakeRunner(Arc<Fakes>);

    impl Runner for FakeRunner {
        fn idle(&self) -> bool {
            self.0.idle.load(Ordering::SeqCst)
        }

        fn start(&self, request: Request) {
            assert!(
                self.0.idle.swap(false, Ordering::SeqCst),
                "started while busy"
            );
            self.0.started.lock().unwrap().push(request);
        }
    }

    struct World {
        fakes: Arc<Fakes>,
        scheduler: Arc<Scheduler>,
    }

    fn at(text: &str) -> DateTime<Local> {
        cron::local(NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap())
    }

    fn world(start: &str, plans: &[(&str, Plan)]) -> World {
        let schedule = |cron: Option<&str>| {
            cron.map(|cron| format!("  schedule: '{cron}'\n"))
                .unwrap_or_default()
        };
        let entries = plans
            .iter()
            .map(|(service, plan)| {
                let yaml = format!(
                    "service: {service}\nhome: /srv/{service}\nbtrfs:\n{}  subvolume: /disk/@{service}\n  snapshots: /disk/.snapshots\n  retention: {{ preserveMin: 24h, preserve: [30d] }}\nrestic:\n{}  repository: s3:x/{service}\n  envset: c\n  retention: {{ keepLast: 1, keepDaily: 1, keepWeekly: 1, keepMonthly: 1 }}\n  backupPaths: [/data]\n",
                    schedule(plan.btrfs),
                    schedule(plan.restic)
                );
                Entry {
                    path: format!("/atlas/{service}.yaml"),
                    config: atlas::parse(&serde_yaml_ng::from_str(&yaml).unwrap()).unwrap(),
                }
            })
            .collect();
        let services = plans
            .iter()
            .map(|(service, plan)| {
                let state = plan
                    .restic_state
                    .unwrap_or(if plan.newest_restic.is_some() {
                        BackendState::Ok
                    } else {
                        BackendState::Absent
                    });
                let info = BackendInfo::Restic {
                    repository: String::new(),
                    envset: String::new(),
                    retention: ResticRetentionInfo {
                        keep_last: 1,
                        keep_daily: 1,
                        keep_weekly: 1,
                        keep_monthly: 1,
                    },
                    paths: vec![],
                    hooks: HooksInfo {
                        prepare: None,
                        release: None,
                        restore: None,
                    },
                };
                Service {
                    name: service.to_string(),
                    backends: BTreeMap::from([(
                        Backend::Restic,
                        BackendStatus {
                            state,
                            message: None,
                            info,
                        },
                    )]),
                    snapshots: plan
                        .newest_restic
                        .map(|time| Snapshot {
                            backend: Backend::Restic,
                            service: service.to_string(),
                            time: iso(at(time).with_timezone(&Utc)),
                            handle: "abc".to_string(),
                            details: SnapshotDetails::Restic {
                                tags: vec![],
                                paths: vec![],
                            },
                        })
                        .into_iter()
                        .collect(),
                }
            })
            .collect();

        let fakes = Arc::new(Fakes {
            entries,
            services,
            idle: AtomicBool::new(true),
            started: Mutex::default(),
            now: Mutex::new(at(start)),
        });
        let clock = fakes.clone();
        let scheduler = Scheduler::with_clock(
            fakes.clone(),
            Box::new(FakeRunner(fakes.clone())),
            Box::new(move || *clock.now.lock().unwrap()),
        );
        World { fakes, scheduler }
    }

    impl World {
        fn start(&self) {
            self.scheduler.catch_up();
            self.scheduler.drain();
        }

        fn at(&self, time: &str) {
            *self.fakes.now.lock().unwrap() = at(time);
            self.scheduler.check();
        }

        fn finish(&self) {
            self.fakes.idle.store(true, Ordering::SeqCst);
            self.scheduler.drain();
        }

        fn occupy(&self) {
            self.fakes.idle.store(false, Ordering::SeqCst);
        }

        fn started(&self) -> Vec<String> {
            self.fakes
                .started
                .lock()
                .unwrap()
                .iter()
                .map(Request::title)
                .collect()
        }
    }

    fn btrfs(cron: &'static str) -> Plan {
        Plan {
            btrfs: Some(cron),
            ..Plan::default()
        }
    }

    #[test]
    fn should_run_the_services_that_are_due_at_the_same_moment_as_one_job() {
        let w = world(
            "2026-10-03T14:50:00",
            &[
                ("wiki", btrfs("0 * * * *")),
                ("mail", btrfs("0 * * * *")),
                ("dns", btrfs("30 * * * *")),
            ],
        );
        w.start();
        w.at("2026-10-03T14:59:40");
        assert_eq!(w.started(), Vec::<String>::new());

        w.at("2026-10-03T15:00:05");
        assert_eq!(w.started(), vec!["Backup · btrfs · mail (hot), wiki (hot)"]);

        w.finish();
        w.at("2026-10-03T15:00:25");
        assert_eq!(w.started().len(), 1);
    }

    #[test]
    fn should_leave_a_backend_without_a_schedule_alone() {
        let w = world("2026-10-03T14:50:00", &[("wiki", Plan::default())]);
        w.start();
        w.at("2026-10-04T14:50:00");
        assert_eq!(w.started(), Vec::<String>::new());
        assert_eq!(w.scheduler.list(), vec![]);
    }

    #[test]
    fn should_keep_a_due_backup_waiting_once_however_long_the_runner_is_busy() {
        let plan = Plan {
            btrfs: Some("0 * * * *"),
            restic: Some("0 3 * * *"),
            ..Plan::default()
        };
        let w = world("2026-10-03T02:50:00", &[("wiki", plan)]);
        w.start();
        w.at("2026-10-03T03:00:10");
        // btrfs goes first; restic waits its turn
        assert_eq!(w.started(), vec!["Backup · btrfs · wiki (hot)"]);
        assert!(
            w.scheduler
                .list()
                .iter()
                .find(|s| s.backend == Backend::Restic)
                .unwrap()
                .waiting
        );
        w.finish();
        assert_eq!(
            w.started(),
            vec!["Backup · btrfs · wiki (hot)", "Backup · restic · wiki"]
        );

        // the restic run takes three hours: btrfs comes due three times and runs once
        w.at("2026-10-03T04:00:10");
        w.at("2026-10-03T05:00:10");
        w.at("2026-10-03T06:00:10");
        w.finish();
        assert_eq!(
            w.started(),
            vec![
                "Backup · btrfs · wiki (hot)",
                "Backup · restic · wiki",
                "Backup · btrfs · wiki (hot)"
            ]
        );
        w.finish();
        assert_eq!(w.started().len(), 3);
    }

    #[test]
    fn should_wait_for_a_job_that_was_started_by_hand() {
        let w = world("2026-10-03T14:50:00", &[("wiki", btrfs("0 * * * *"))]);
        w.start();
        w.occupy();
        w.at("2026-10-03T15:00:10");
        assert_eq!(w.started(), Vec::<String>::new());
        w.finish();
        assert_eq!(w.started(), vec!["Backup · btrfs · wiki (hot)"]);
    }

    #[test]
    fn should_catch_up_at_startup_on_a_run_that_was_missed() {
        let restic = |newest| Plan {
            restic: Some("0 3 * * *"),
            newest_restic: Some(newest),
            ..Plan::default()
        };
        let w = world(
            "2026-10-03T09:30:00",
            &[
                ("late", restic("2026-10-02T03:01:00")),
                ("fine", restic("2026-10-03T03:01:00")),
            ],
        );
        w.start();
        assert_eq!(w.started(), vec!["Backup · restic · late"]);
    }

    #[test]
    fn should_not_catch_up_without_history_or_without_knowing_the_history() {
        let fresh = Plan {
            restic: Some("0 3 * * *"),
            ..Plan::default()
        };
        let unknown = Plan {
            restic: Some("0 3 * * *"),
            newest_restic: Some("2026-09-01T03:01:00"),
            restic_state: Some(BackendState::Error),
            ..Plan::default()
        };
        let w = world(
            "2026-10-03T09:30:00",
            &[("fresh", fresh), ("unknown", unknown)],
        );
        w.start();
        assert_eq!(w.started(), Vec::<String>::new());

        // their next regular slot still fires
        w.at("2026-10-04T03:00:10");
        assert_eq!(w.started(), vec!["Backup · restic · fresh, unknown"]);
    }

    #[test]
    fn should_list_every_schedule_with_its_next_moment() {
        let plan = Plan {
            btrfs: Some("0 * * * *"),
            restic: Some("0 3 * * *"),
            ..Plan::default()
        };
        let w = world("2026-10-03T09:30:00", &[("wiki", plan)]);
        let next = |time: &str| Some(iso(at(time).with_timezone(&Utc)));
        assert_eq!(
            w.scheduler.list(),
            vec![
                Schedule {
                    service: "wiki".into(),
                    backend: Backend::Btrfs,
                    cron: "0 * * * *".into(),
                    next: next("2026-10-03T10:00:00"),
                    waiting: false
                },
                Schedule {
                    service: "wiki".into(),
                    backend: Backend::Restic,
                    cron: "0 3 * * *".into(),
                    next: next("2026-10-04T03:00:00"),
                    waiting: false
                },
            ]
        );
    }

    #[test]
    fn should_start_nothing_once_stopped() {
        let w = world("2026-10-03T14:50:00", &[("wiki", btrfs("0 * * * *"))]);
        w.start();
        w.scheduler.stop();
        w.at("2026-10-03T15:00:10");
        assert_eq!(w.started(), Vec::<String>::new());
    }
}
