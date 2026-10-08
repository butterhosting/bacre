use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::future::join_all;

use super::atlas_service::{AtlasService, Scan};
use super::change_service::ChangeService;
use crate::backends::{self, Context};
use crate::models::archives::{Backend, BackendState, BackendStatus, Problem, Service, iso};
use crate::models::atlas::Entry;

const REFRESH: Duration = Duration::from_secs(30 * 60);
const ATLAS_CHECK: Duration = Duration::from_secs(10);
const STALE: chrono::Duration = chrono::Duration::minutes(5);

pub struct ArchiveService {
    ctx: Context,
    atlas_service: AtlasService,
    change_service: Arc<ChangeService>,
    state: Mutex<State>,
    /// Held for a whole scan, so one asked for while another runs waits its turn rather than
    /// trusting a scan that may have started before the caller's change
    scan: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct State {
    atlas: Vec<Entry>,
    services: Vec<Service>,
    refreshed_at: Option<DateTime<Utc>>,
    errors: Vec<Problem>,
    refreshing: bool,
    scanned: Option<Scan>,
}

pub struct View {
    pub refreshed_at: Option<String>,
    pub refreshing: bool,
    pub errors: Vec<Problem>,
    pub services: Vec<Service>,
}

impl ArchiveService {
    pub fn new(
        ctx: Context,
        atlas_service: AtlasService,
        change_service: Arc<ChangeService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            atlas_service,
            change_service,
            state: Mutex::default(),
            scan: tokio::sync::Mutex::default(),
        })
    }

    pub async fn start(self: &Arc<Self>) {
        self.refresh().await;
        {
            let state = self.state.lock().unwrap();
            println!(
                "==> Initial refresh done: {} services, {} errors",
                state.services.len(),
                state.errors.len()
            );
        }
        self.every(REFRESH, |service| async move { service.refresh().await });
        self.every(ATLAS_CHECK, |service| async move {
            service.refresh_if_atlas_changed().await
        });
    }

    pub fn view(&self) -> View {
        let state = self.state.lock().unwrap();
        View {
            refreshed_at: state.refreshed_at.map(iso),
            refreshing: state.refreshing,
            errors: state.errors.clone(),
            services: state.services.clone(),
        }
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.state.lock().unwrap().atlas.clone()
    }

    pub fn entry(&self, service: &str) -> Option<Entry> {
        self.state
            .lock()
            .unwrap()
            .atlas
            .iter()
            .find(|entry| entry.config.service == service)
            .cloned()
    }

    pub fn services(&self) -> Vec<Service> {
        self.state.lock().unwrap().services.clone()
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub fn refresh_if_stale(self: &Arc<Self>) {
        let stale = {
            let state = self.state.lock().unwrap();
            !state.refreshing && state.refreshed_at.is_some_and(|at| Utc::now() - at > STALE)
        };
        if stale {
            let service = self.clone();
            tokio::spawn(async move { service.refresh().await });
        }
    }

    async fn refresh_if_atlas_changed(&self) {
        if self.state.lock().unwrap().refreshing {
            return;
        }
        let scan = self.atlas_service.scan().await;
        if self.state.lock().unwrap().scanned.as_ref() != Some(&scan) {
            println!("==> The atlas changed, refreshing");
            self.refresh().await;
        }
    }

    pub async fn refresh(&self) {
        let _scan = self.scan.lock().await;
        self.state.lock().unwrap().refreshing = true;
        self.change_service.changed();

        let scan = self.atlas_service.scan().await;
        let listed = join_all(scan.entries.iter().map(|entry| self.list_service(entry))).await;
        let mut errors: Vec<Problem> = scan
            .problems
            .iter()
            .map(|problem| Problem {
                source: problem.path.clone(),
                message: problem.message.clone(),
            })
            .collect();
        let mut services = Vec::new();
        for (service, problems) in listed {
            services.push(service);
            errors.extend(problems);
        }

        {
            let mut state = self.state.lock().unwrap();
            state.atlas = scan.entries.clone();
            state.services = services;
            state.refreshed_at = Some(Utc::now());
            state.errors = errors;
            state.scanned = Some(scan);
            state.refreshing = false;
        }
        self.change_service.changed();
    }

    async fn list_service(&self, entry: &Entry) -> (Service, Vec<Problem>) {
        let name = entry.config.service.clone();
        let mut service_backends = BTreeMap::new();
        let mut snapshots = Vec::new();
        let mut problems = Vec::new();
        for backend in Backend::ALL {
            if !entry.config.configures(backend) {
                continue;
            }
            match backends::list(&self.ctx, entry, backend).await {
                Ok(listing) => {
                    let state = if listing.found {
                        BackendState::Ok
                    } else {
                        BackendState::Absent
                    };
                    service_backends.insert(
                        backend,
                        BackendStatus {
                            state,
                            message: None,
                            info: listing.info,
                        },
                    );
                    snapshots.extend(listing.snapshots);
                }
                Err(message) => {
                    problems.push(Problem {
                        source: format!("{backend} · {name}"),
                        message: message.clone(),
                    });
                    if let Ok(info) = backends::describe(entry, backend) {
                        service_backends.insert(
                            backend,
                            BackendStatus {
                                state: BackendState::Error,
                                message: Some(message),
                                info,
                            },
                        );
                    }
                }
            }
        }
        snapshots.sort_by(|a, b| b.time.cmp(&a.time));
        (
            Service {
                name,
                backends: service_backends,
                snapshots,
            },
            problems,
        )
    }

    fn every<F>(self: &Arc<Self>, period: Duration, work: impl Fn(Arc<Self>) -> F + Send + 'static)
    where
        F: Future<Output = ()> + Send,
    {
        let service = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                let Some(service) = service.upgrade() else {
                    break;
                };
                work(service).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use tokio::sync::{Notify, Semaphore};

    use super::*;
    use crate::config::Config;
    use crate::shell::{Env, OnLine, Output, Shell};

    /// Every restic call waits at the gate, so a scan can be caught half way
    struct GatedShell {
        entered: Notify,
        gate: Semaphore,
    }

    impl Default for GatedShell {
        fn default() -> Self {
            Self {
                entered: Notify::new(),
                gate: Semaphore::new(0),
            }
        }
    }

    #[async_trait]
    impl Shell for GatedShell {
        async fn run(&self, _cmd: &[String], _env: &Env) -> Output {
            self.entered.notify_one();
            self.gate.acquire().await.unwrap().forget();
            Output {
                code: 1,
                stdout: String::new(),
                stderr: "Fatal: unable to open repository".to_string(),
            }
        }

        async fn stream(&self, _: &[String], _: Option<&str>, _: &Env, _: OnLine<'_>) -> i32 {
            unreachable!("a refresh only lists")
        }

        async fn pipe(&self, _: &[String], _: &[String], _: &Env, _: OnLine<'_>) -> i32 {
            unreachable!("a refresh only lists")
        }
    }

    fn write_service(dir: &std::path::Path, name: &str) {
        std::fs::write(
            dir.join(format!("{name}.yaml")),
            format!("service: {name}\nhome: /srv/{name}\nrestic:\n  repository: /repos/{name}\n  envset: demo\n  retention: {{ keepLast: 3 }}\n  backupPaths: [/srv/{name}/data]\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn should_scan_again_when_asked_while_a_scan_from_before_a_change_runs() {
        // given
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(
            Config::parse(&format!(
                "server: {{ bind: 127.0.0.1, port: 3001 }}\nservices: ['{}/*.yaml']\nenvsets: {{ demo: {{}} }}\nbackends: {{ restic: {{ cacheDir: /cache, stagingDir: /staging }} }}",
                dir.path().display()
            ))
            .unwrap(),
        );
        let shell = Arc::new(GatedShell::default());
        let service = ArchiveService::new(
            Context {
                config: config.clone(),
                shell: shell.clone(),
            },
            AtlasService::new(config),
            Arc::new(ChangeService::default()),
        );
        write_service(dir.path(), "first");
        let before = tokio::spawn({
            let service = service.clone();
            async move { service.refresh().await }
        });
        shell.entered.notified().await;

        // when
        write_service(dir.path(), "second");
        let after = tokio::spawn({
            let service = service.clone();
            async move { service.refresh().await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        shell.gate.add_permits(Semaphore::MAX_PERMITS);
        before.await.unwrap();
        after.await.unwrap();

        // then
        let names: Vec<String> = service
            .view()
            .services
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["first", "second"]);
        assert!(!service.view().refreshing);
    }
}
