//! Runs one job at a time and keeps the recent ones in memory, so a page can follow a job
//! live or come back to it later. Nothing is persisted: the archives are the history.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tokio::sync::{mpsc, watch};

use super::log::Log;
use crate::failure::Outcome;
use crate::models::archives::iso;
use crate::models::jobs::{Event, Job, Line, LineStream, Request, Status, Summary, Trigger};

/// Turns a job request into the work it stands for
pub type Executor =
    Arc<dyn Fn(Request, Log) -> Pin<Box<dyn Future<Output = Outcome> + Send>> + Send + Sync>;

/// Another job is still running
#[derive(Debug, Clone, PartialEq)]
pub struct Busy {
    pub job_id: String,
}

/// Callbacks registered while the daemon is wired together
type Listeners<F> = Mutex<Vec<Box<F>>>;

/// How many jobs are remembered
const KEEP: usize = 50;

pub struct JobService {
    executor: Executor,
    state: Mutex<State>,
    idle: watch::Sender<bool>,
    on_changed: Listeners<dyn Fn() + Send + Sync>,
    on_finished: Listeners<dyn Fn(&Job) + Send + Sync>,
    on_idle: Listeners<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct State {
    /// In the order they were started
    jobs: Vec<Tracked>,
    running: Option<String>,
    /// Someone is rearranging the sandbox (seed, purge): no job may start meanwhile
    held: bool,
}

/// What a refused start says while the sandbox is being seeded or purged
pub const HELD: &str = "sandbox";

/// Keeps every job from starting for as long as it lives
pub struct Hold {
    service: Arc<JobService>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.service.state.lock().unwrap().held = false;
        // whatever became due meanwhile can go now
        self.service.tell(&self.service.on_idle);
    }
}

struct Tracked {
    job: Job,
    subscribers: Vec<mpsc::UnboundedSender<Event>>,
}

impl JobService {
    pub fn new(executor: Executor) -> Arc<Self> {
        Arc::new(Self {
            executor,
            state: Mutex::default(),
            idle: watch::Sender::new(true),
            on_changed: Mutex::default(),
            on_finished: Mutex::default(),
            on_idle: Mutex::default(),
        })
    }

    /// Starts the job and returns at once; refuses while another job runs
    pub fn start(self: &Arc<Self>, request: Request, trigger: Trigger) -> Result<Job, Busy> {
        let job = {
            let mut state = self.state.lock().unwrap();
            if let Some(job_id) = state.running.clone() {
                return Err(Busy { job_id });
            }
            if state.held {
                return Err(Busy {
                    job_id: HELD.to_string(),
                });
            }
            let job = Job {
                id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
                trigger,
                title: request.title(),
                request: request.clone(),
                status: Status::Running,
                started_at: iso(Utc::now()),
                ended_at: None,
                error: None,
                failed: Vec::new(),
                lines: Vec::new(),
            };
            state.running = Some(job.id.clone());
            state.jobs.push(Tracked {
                job: job.clone(),
                subscribers: Vec::new(),
            });
            trim(&mut state);
            job
        };
        self.idle.send_replace(false);
        self.tell(&self.on_changed);

        let log = {
            let (service, id) = (Arc::downgrade(self), job.id.clone());
            Log::new(move |stream, text| {
                if let Some(service) = service.upgrade() {
                    service.append(&id, stream, text);
                }
            })
        };
        let (service, id) = (self.clone(), job.id.clone());
        let work = (self.executor)(request, log);
        tokio::spawn(async move {
            let outcome = work.await;
            service.finish(&id, outcome);
        });
        Ok(job)
    }

    pub fn idle(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.running.is_none() && !state.held
    }

    /// Keeps jobs from starting until the hold is dropped; refused while one runs
    pub fn hold(self: &Arc<Self>) -> Result<Hold, Busy> {
        let mut state = self.state.lock().unwrap();
        if let Some(job_id) = state.running.clone() {
            return Err(Busy { job_id });
        }
        if state.held {
            return Err(Busy {
                job_id: HELD.to_string(),
            });
        }
        state.held = true;
        Ok(Hold {
            service: self.clone(),
        })
    }

    /// Forgets every job (a purged sandbox has no history). Only while held, so none runs.
    pub fn forget_all(&self, _hold: &Hold) {
        self.state.lock().unwrap().jobs.clear();
        self.tell(&self.on_changed);
    }

    /// Called whenever the list of jobs looks different: one started, one ended
    pub fn on_changed(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.on_changed.lock().unwrap().push(Box::new(listener));
    }

    /// Called with every job that ends, whatever started it and however it went
    pub fn on_finished(&self, listener: impl Fn(&Job) + Send + Sync + 'static) {
        self.on_finished.lock().unwrap().push(Box::new(listener));
    }

    /// Called every time a job ends, once the runner is free again
    pub fn on_idle(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.on_idle.lock().unwrap().push(Box::new(listener));
    }

    /// Returns when no job runs (at once, when none does)
    pub async fn settled(&self) {
        let mut idle = self.idle.subscribe();
        // the sender lives as long as the service, so this cannot fail
        let _ = idle.wait_for(|idle| *idle).await;
    }

    /// Adds a finished job as if it had run here (the dev stage's sample data)
    pub fn seed(&self, job: Job) {
        self.state.lock().unwrap().jobs.push(Tracked {
            job,
            subscribers: Vec::new(),
        });
    }

    /// Newest first, without their lines
    pub fn list(&self) -> Vec<Summary> {
        let state = self.state.lock().unwrap();
        let mut jobs: Vec<Summary> = state
            .jobs
            .iter()
            .map(|tracked| Summary::from(&tracked.job))
            .collect();
        jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        jobs
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .iter()
            .find(|tracked| tracked.job.id == id)
            .map(|tracked| tracked.job.clone())
    }

    /// Everything the job has printed so far, and then what it prints until it ends, in one
    /// stream that closes after `Done`. Taken under the lock, so no line can slip between
    /// the replay and the live feed. Nothing for a job that is not known.
    pub fn subscribe(&self, id: &str) -> Option<mpsc::UnboundedReceiver<Event>> {
        let mut state = self.state.lock().unwrap();
        let tracked = state.jobs.iter_mut().find(|tracked| tracked.job.id == id)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        for line in &tracked.job.lines {
            let _ = sender.send(Event::Line(line.clone()));
        }
        if tracked.job.status == Status::Running {
            tracked.subscribers.push(sender);
        } else {
            let _ = sender.send(Event::Done {
                status: tracked.job.status,
                error: tracked.job.error.clone(),
            });
        }
        Some(receiver)
    }

    fn append(&self, id: &str, stream: LineStream, text: String) {
        let mut state = self.state.lock().unwrap();
        if let Some(tracked) = state.jobs.iter_mut().find(|tracked| tracked.job.id == id) {
            append(tracked, stream, text);
        }
    }

    fn finish(&self, id: &str, outcome: Outcome) {
        let finished = {
            let mut state = self.state.lock().unwrap();
            state.running = None;
            let Some(tracked) = state.jobs.iter_mut().find(|tracked| tracked.job.id == id) else {
                return;
            };
            tracked.job.ended_at = Some(iso(Utc::now()));
            match outcome {
                Ok(()) => {
                    tracked.job.status = Status::Succeeded;
                    append(tracked, LineStream::Info, "==> Done".to_string());
                }
                Err(failure) => {
                    tracked.job.status = Status::Failed;
                    append(
                        tracked,
                        LineStream::Info,
                        format!("==> Failed: {}", failure.message),
                    );
                    tracked.job.error = Some(failure.message);
                    tracked.job.failed = failure.services;
                }
            }
            let done = Event::Done {
                status: tracked.job.status,
                error: tracked.job.error.clone(),
            };
            for subscriber in tracked.subscribers.drain(..) {
                let _ = subscriber.send(done.clone());
            }
            tracked.job.clone()
        };
        self.tell(&self.on_changed);
        for listener in self.on_finished.lock().unwrap().iter() {
            listener(&finished);
        }
        self.idle.send_replace(true);
        self.tell(&self.on_idle);
    }

    fn tell(&self, listeners: &Listeners<dyn Fn() + Send + Sync>) {
        for listener in listeners.lock().unwrap().iter() {
            listener();
        }
    }
}

fn append(tracked: &mut Tracked, stream: LineStream, text: String) {
    let line = Line {
        at: iso(Utc::now()),
        stream,
        text,
    };
    tracked
        .subscribers
        .retain(|subscriber| subscriber.send(Event::Line(line.clone())).is_ok());
    tracked.job.lines.push(line);
}

/// Forgets the oldest finished jobs beyond the cap
fn trim(state: &mut State) {
    while state.jobs.len() > KEEP {
        let oldest = state
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, tracked)| tracked.job.status != Status::Running)
            .min_by(|(_, a), (_, b)| a.job.started_at.cmp(&b.job.started_at))
            .map(|(index, _)| index);
        match oldest {
            Some(index) => {
                state.jobs.remove(index);
            }
            None => break,
        }
    }
}

#[cfg(test)]
pub mod testing {
    use std::sync::{Arc, Mutex};

    use tokio::sync::oneshot;

    use super::*;
    use crate::failure::Failure;

    /// An executor the test drives by hand
    #[derive(Clone, Default)]
    pub struct Controllable {
        current: Arc<Mutex<Option<Running>>>,
    }

    /// How to end the running job, and where it writes
    type Running = (oneshot::Sender<Outcome>, Log);

    impl Controllable {
        pub fn executor(&self) -> Executor {
            let current = self.current.clone();
            Arc::new(move |_request, log| {
                let (sender, receiver) = oneshot::channel();
                *current.lock().unwrap() = Some((sender, log));
                Box::pin(async move { receiver.await.unwrap_or(Ok(())) })
            })
        }

        pub fn log(&self) -> Log {
            self.current
                .lock()
                .unwrap()
                .as_ref()
                .expect("a job is running")
                .1
                .clone()
        }

        pub fn finish(&self) {
            self.end(Ok(()));
        }

        pub fn fail(&self, message: &str) {
            self.end(Err(Failure::new(message)));
        }

        pub fn end_with(&self, failure: Failure) {
            self.end(Err(failure));
        }

        fn end(&self, outcome: Outcome) {
            let (sender, _) = self
                .current
                .lock()
                .unwrap()
                .take()
                .expect("a job is running");
            let _ = sender.send(outcome);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::testing::Controllable;
    use super::*;
    use crate::models::jobs::{BackupRequest, BtrfsTarget, Mode};

    fn request(mode: Mode) -> Request {
        Request::Backup(BackupRequest::Btrfs {
            targets: vec![BtrfsTarget {
                service: "wiki".to_string(),
                mode,
            }],
        })
    }

    fn events(receiver: &mut mpsc::UnboundedReceiver<Event>) -> Vec<String> {
        let mut seen = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            seen.push(match event {
                Event::Line(line) => line.text,
                Event::Done { status, .. } => format!("done:{status:?}"),
            });
        }
        seen
    }

    #[tokio::test]
    async fn should_start_a_job_and_refuse_a_second_one_while_it_runs() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());

        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        assert_eq!(job.status, Status::Running);
        assert_eq!(job.title, "Backup · btrfs · wiki (hot)");
        assert_eq!(
            service.start(request(Mode::Hot), Trigger::Manual),
            Err(Busy {
                job_id: job.id.clone()
            })
        );

        control.finish();
        service.settled().await;
        assert_eq!(service.get(&job.id).unwrap().status, Status::Succeeded);
        assert!(service.start(request(Mode::Hot), Trigger::Manual).is_ok());
    }

    #[tokio::test]
    async fn should_record_the_failure_and_keep_the_lines() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());

        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        control.log().out("hello");
        control.fail("boom");
        service.settled().await;

        let finished = service.get(&job.id).unwrap();
        assert_eq!(finished.status, Status::Failed);
        assert_eq!(finished.error.as_deref(), Some("boom"));
        assert!(finished.ended_at.is_some());
        let lines: Vec<(LineStream, &str)> = finished
            .lines
            .iter()
            .map(|line| (line.stream, line.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (LineStream::Out, "hello"),
                (LineStream::Info, "==> Failed: boom")
            ]
        );
    }

    #[tokio::test]
    async fn should_replay_earlier_lines_to_a_late_subscriber_and_then_stream_the_rest() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        control.log().info("one");

        let mut receiver = service.subscribe(&job.id).unwrap();
        control.log().out("two");
        control.finish();
        service.settled().await;

        assert_eq!(
            events(&mut receiver),
            vec!["one", "two", "==> Done", "done:Succeeded"]
        );
        // the stream is closed once the job is done
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn should_tell_a_subscriber_of_a_finished_job_everything_at_once() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        control.finish();
        service.settled().await;

        let mut receiver = service.subscribe(&job.id).unwrap();

        assert_eq!(
            events(&mut receiver).last().map(String::as_str),
            Some("done:Succeeded")
        );
        assert!(service.subscribe("unknown").is_none());
    }

    #[tokio::test]
    async fn should_list_newest_first_without_lines() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let first = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        control.log().out("x");
        control.finish();
        service.settled().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = service.start(request(Mode::Cold), Trigger::Manual).unwrap();

        let list = service.list();
        assert_eq!(
            list.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
            vec![second.id.as_str(), first.id.as_str()]
        );
        assert!(
            serde_json::to_value(&list[1])
                .unwrap()
                .get("lines")
                .is_none()
        );
    }

    #[tokio::test]
    async fn should_say_who_started_a_job_and_tell_its_listeners_when_the_runner_is_free_again() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let freed = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(Mutex::new(Vec::new()));
        service.on_idle({
            let freed = freed.clone();
            move || {
                freed.fetch_add(1, Ordering::SeqCst);
            }
        });
        service.on_finished({
            let finished = finished.clone();
            move |job| finished.lock().unwrap().push((job.id.clone(), job.status))
        });

        assert!(service.idle());
        let manual = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        assert_eq!(manual.trigger, Trigger::Manual);
        control.finish();
        service.settled().await;
        assert_eq!(freed.load(Ordering::SeqCst), 1);

        let scheduled = service
            .start(request(Mode::Hot), Trigger::Schedule)
            .unwrap();
        assert!(!service.idle());
        assert_eq!(
            service
                .list()
                .iter()
                .find(|job| job.id == scheduled.id)
                .unwrap()
                .trigger,
            Trigger::Schedule
        );
        control.fail("nope");
        service.settled().await;

        assert_eq!(freed.load(Ordering::SeqCst), 2);
        assert_eq!(
            *finished.lock().unwrap(),
            vec![
                (manual.id, Status::Succeeded),
                (scheduled.id, Status::Failed)
            ]
        );
    }

    #[tokio::test]
    async fn should_remember_which_services_a_failure_names() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        control.end_with(crate::failure::Failure::of(
            "1 of 2 failed: wiki",
            vec!["wiki".to_string()],
        ));
        service.settled().await;

        assert_eq!(service.get(&job.id).unwrap().failed, vec!["wiki"]);
    }

    #[tokio::test]
    async fn should_start_nothing_while_held_and_refuse_a_hold_while_a_job_runs() {
        let control = Controllable::default();
        let service = JobService::new(control.executor());
        let freed = Arc::new(AtomicUsize::new(0));
        service.on_idle({
            let freed = freed.clone();
            move || {
                freed.fetch_add(1, Ordering::SeqCst);
            }
        });

        let job = service.start(request(Mode::Hot), Trigger::Manual).unwrap();
        assert_eq!(
            service.hold().err(),
            Some(Busy {
                job_id: job.id.clone()
            })
        );
        control.finish();
        service.settled().await;

        let hold = service.hold().unwrap();
        assert!(!service.idle());
        assert_eq!(
            service.start(request(Mode::Hot), Trigger::Manual).err(),
            Some(Busy {
                job_id: HELD.to_string()
            })
        );
        assert_eq!(
            service.hold().err(),
            Some(Busy {
                job_id: HELD.to_string()
            })
        );
        service.forget_all(&hold);
        assert_eq!(service.list(), vec![]);

        let before = freed.load(Ordering::SeqCst);
        drop(hold);
        assert!(service.idle());
        // released like a job that ended, so whatever waits can go
        assert_eq!(freed.load(Ordering::SeqCst), before + 1);
        assert!(service.start(request(Mode::Hot), Trigger::Manual).is_ok());
    }
}
