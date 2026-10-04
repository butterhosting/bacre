mod backends;
mod build_info;
mod config;
mod cron;
mod failure;
mod jobs;
mod models;
mod server;
mod services;
mod shell;

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;

use crate::backends::Context;
use crate::config::{Config, Stage};
use crate::jobs::dev_jobs;
use crate::jobs::job_executor::JobExecutor;
use crate::jobs::job_service::JobService;
use crate::services::archive_service::ArchiveService;
use crate::services::atlas_service::AtlasService;
use crate::services::change_service::ChangeService;
use crate::services::sandbox_service::SandboxService;
use crate::services::scheduler::Scheduler;
use crate::services::webhook_service::WebhookService;
use crate::shell::{RealShell, SandboxShell, Shell};

#[tokio::main]
async fn main() -> ExitCode {
    let argument = std::env::args().nth(1);
    let config_path = match argument.as_deref() {
        Some("--version" | "-V") => {
            println!("bacre {} ({})", build_info::VERSION, build_info::commit());
            return ExitCode::SUCCESS;
        }
        Some(path) => path.to_string(),
        None => {
            eprintln!("usage: bacre <config.yaml>");
            return ExitCode::from(2);
        }
    };
    match run(Path::new(&config_path)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config_path: &Path) -> Result<(), String> {
    let config = Arc::new(Config::load(config_path)?);
    let shell: Arc<dyn Shell> = match config.stage {
        Stage::Dev | Stage::E2e => Arc::new(SandboxShell::default()),
        Stage::Prod => Arc::new(RealShell),
    };

    let change_service = Arc::new(ChangeService::default());
    let ctx = Context {
        config: config.clone(),
        shell,
    };
    let archive_service = ArchiveService::new(
        ctx.clone(),
        AtlasService::new(config.clone()),
        change_service.clone(),
    );
    let job_service = JobService::new(JobExecutor::new(archive_service.clone()).executor());
    let scheduler = Scheduler::new(archive_service.clone(), Box::new(job_service.clone()));
    let webhook_service = Arc::new(WebhookService::new(config.clone()));
    let sandbox_service = SandboxService::new(
        ctx,
        job_service.clone(),
        scheduler.clone(),
        archive_service.clone(),
    );

    job_service.on_changed({
        let change_service = change_service.clone();
        move || change_service.changed()
    });
    job_service.on_idle({
        let scheduler = Arc::downgrade(&scheduler);
        move || {
            if let Some(scheduler) = scheduler.upgrade() {
                scheduler.drain();
            }
        }
    });
    // kept, so a restart can wait for the last report to go out
    let reports: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
    job_service.on_finished({
        let reports = reports.clone();
        move |job| {
            let (webhook_service, job) = (webhook_service.clone(), job.clone());
            let mut reports = reports.lock().unwrap();
            reports.retain(|report| !report.is_finished());
            reports.push(tokio::spawn(
                async move { webhook_service.notify(&job).await },
            ));
        }
    });

    let address: SocketAddr = format!("{}:{}", config.server.bind, config.server.port)
        .parse()
        .map_err(|e| format!("server.bind and server.port do not make an address: {e}"))?;
    let listener = TcpListener::bind(address)
        .await
        .map_err(|e| format!("could not listen on {address}: {e}"))?;
    banner(&config, address);
    let app = Arc::new(server::App {
        config: config.clone(),
        archive_service: archive_service.clone(),
        job_service: job_service.clone(),
        scheduler: scheduler.clone(),
        change_service,
        sandbox: sandbox_service.clone(),
    });

    tokio::spawn({
        let (config, job_service, scheduler) =
            (config.clone(), job_service.clone(), scheduler.clone());
        async move {
            match config.stage {
                Stage::Dev => {
                    if let Some(sandbox) = &sandbox_service {
                        sandbox.seed_if_empty().await;
                    }
                    archive_service.start().await;
                    dev_jobs::seed(&job_service);
                    scheduler.start();
                }
                // the schedules are listed, but nothing fires: a test decides what runs
                Stage::E2e => archive_service.start().await,
                Stage::Prod => {
                    archive_service.start().await;
                    scheduler.start();
                }
            }
        }
    });

    tokio::select! {
        served = axum::serve(listener, server::router(app)) => served.map_err(|e| e.to_string()),
        () = leave(&config, &job_service, &scheduler, &reports) => Ok(()),
    }
}

/// A restart must not cut a job short (a cold snapshot or a restore has its service stopped):
/// nothing new starts, and the daemon leaves once the running job and its report are done.
async fn leave(
    config: &Config,
    job_service: &JobService,
    scheduler: &Scheduler,
    reports: &Mutex<Vec<JoinHandle<()>>>,
) {
    let mut terminate = signal(SignalKind::terminate()).expect("could not listen for SIGTERM");
    let mut interrupt = signal(SignalKind::interrupt()).expect("could not listen for SIGINT");
    let name = tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
    };
    if config.stage != Stage::Prod {
        return;
    }
    scheduler.stop();
    if job_service.idle() {
        println!("==> {name}: leaving");
    } else {
        println!("==> {name}: leaving once the running job has ended");
    }
    job_service.settled().await;
    let reports: Vec<JoinHandle<()>> = std::mem::take(&mut *reports.lock().unwrap());
    for report in reports {
        let _ = report.await;
    }
}

fn banner(config: &Config, address: SocketAddr) {
    println!(
        "==> Bacre {} ({}), {} stage",
        build_info::VERSION,
        build_info::commit(),
        config.stage.as_str()
    );
    println!("==> Listening on http://{address}");
    if config.users.is_empty() {
        println!("==> Basic auth is OFF: no users in the daemon config");
    } else {
        let names: Vec<&str> = config
            .users
            .iter()
            .map(|user| user.username.as_str())
            .collect();
        println!("==> Basic auth is on, for {}", names.join(", "));
    }
}
