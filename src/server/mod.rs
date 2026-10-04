//! The page itself is public and the data behind it is not: a browser asks for the password
//! on the API's 401.

mod auth;
mod website;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router, middleware};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;
use serde_json::json;
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};

use crate::build_info;
use crate::config::Config;
use crate::jobs::job_service::JobService;
use crate::models::archives::Archives;
use crate::models::atlas::is_service_name;
use crate::models::jobs::{Event, Request, Trigger, is_handle};
use crate::services::archive_service::ArchiveService;
use crate::services::change_service::ChangeService;
use crate::services::sandbox_service::{Refusal, SandboxService};
use crate::services::scheduler::Scheduler;
use crate::services::staging_service;

pub struct App {
    pub config: Arc<Config>,
    pub archive_service: Arc<ArchiveService>,
    pub job_service: Arc<JobService>,
    pub scheduler: Arc<Scheduler>,
    pub change_service: Arc<ChangeService>,
    pub sandbox: Option<Arc<SandboxService>>,
}

const FAVICON: &[u8] = include_bytes!("../../website/src/images/favicon.svg");

pub fn router(app: Arc<App>) -> Router {
    let users = auth::Users::new(&app.config);
    let api = Router::new()
        .route("/env", get(env))
        .route("/archives", get(archives))
        .route("/changes", get(changes))
        .route("/staged/{service}/{handle}", delete(discard))
        .route("/jobs", get(jobs).post(start_job))
        .route("/jobs/{id}", get(job))
        .route("/jobs/{id}/events", get(job_events))
        .route("/restricted/seed", post(seed))
        .route("/restricted/purge", post(purge))
        // an unknown API path is an error of its own, not a page of the website
        .fallback(route_not_found)
        .layer(middleware::from_fn_with_state(users, auth::guard))
        .with_state(app);

    Router::new()
        .route("/health", get(health))
        // a stable address, whatever name the bundler gives the file it links from the page
        .route("/favicon.svg", get(favicon))
        .nest("/api", api)
        .fallback(website::serve)
}

fn problem(status: StatusCode, error: &'static str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

async fn route_not_found() -> Response {
    problem(StatusCode::NOT_FOUND, "route_not_found")
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn favicon() -> Response {
    ([(header::CONTENT_TYPE, "image/svg+xml")], FAVICON).into_response()
}

#[derive(Serialize)]
struct Env {
    stage: &'static str,
    version: &'static str,
    commit: &'static str,
}

async fn env(State(app): State<Arc<App>>) -> Json<Env> {
    Json(Env {
        stage: app.config.stage.as_str(),
        version: build_info::VERSION,
        commit: build_info::commit(),
    })
}

async fn archives(State(app): State<Arc<App>>) -> Json<Archives> {
    app.archive_service.refresh_if_stale();
    let view = app.archive_service.view();
    Json(Archives {
        refreshed_at: view.refreshed_at,
        refreshing: view.refreshing,
        errors: view.errors,
        services: view.services,
        staged: staging_service::list(&app.config).await,
        schedules: app.scheduler.list(),
    })
}

async fn changes(
    State(app): State<Arc<App>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let connected = stream::once(async { SseEvent::default().comment("connected") });
    // a listener that fell behind missed changes: one `changed` covers them all
    let changed = BroadcastStream::new(app.change_service.subscribe())
        .map(|_| SseEvent::default().event("changed").data("{}"));
    Sse::new(connected.chain(changed).map(Ok)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(25))
            .text("still here"),
    )
}

async fn discard(
    State(app): State<Arc<App>>,
    Path((service, handle)): Path<(String, String)>,
) -> Response {
    // both end up in a path below the staging directory
    if !is_service_name(&service) || !is_handle(&handle) {
        return problem(StatusCode::BAD_REQUEST, "invalid_request");
    }
    match staging_service::discard(&app.config, &service, &handle).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            eprintln!("==> Could not discard {service}/{handle}: {e}");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "unknown")
        }
    }
}

async fn jobs(State(app): State<Arc<App>>) -> Response {
    Json(app.job_service.list()).into_response()
}

async fn start_job(State(app): State<Arc<App>>, body: Bytes) -> Response {
    let request = serde_json::from_slice::<Request>(&body)
        .map_err(|e| e.to_string())
        .and_then(|request| request.validate().map(|()| request));
    let request = match request {
        Ok(request) => request,
        Err(issue) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_request", "issues": [issue] })),
            )
                .into_response();
        }
    };
    match app.job_service.start(request, Trigger::Manual) {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(busy) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "busy", "jobId": busy.job_id })),
        )
            .into_response(),
    }
}

async fn job(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    match app.job_service.get(&id) {
        Some(job) => Json(job).into_response(),
        None => problem(StatusCode::NOT_FOUND, "job_not_found"),
    }
}

async fn seed(State(app): State<Arc<App>>) -> Response {
    match &app.sandbox {
        Some(sandbox) => restricted(sandbox.seed().await),
        None => route_not_found().await,
    }
}

async fn purge(State(app): State<Arc<App>>) -> Response {
    match &app.sandbox {
        Some(sandbox) => restricted(sandbox.purge().await),
        None => route_not_found().await,
    }
}

fn restricted(result: Result<(), Refusal>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(Refusal::Busy(busy)) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "busy", "jobId": busy.job_id })),
        )
            .into_response(),
        Err(Refusal::Failed(message)) => {
            eprintln!("==> {message}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "sandbox_failed", "message": message })),
            )
                .into_response()
        }
    }
}

async fn job_events(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let Some(receiver) = app.job_service.subscribe(&id) else {
        return problem(StatusCode::NOT_FOUND, "job_not_found");
    };
    let events = UnboundedReceiverStream::new(receiver).map(|event| {
        Ok::<_, Infallible>(match event {
            Event::Line(line) => SseEvent::default()
                .event("line")
                .data(serde_json::to_string(&line).unwrap_or_default()),
            Event::Done { status, error } => SseEvent::default()
                .event("done")
                .data(json!({ "status": status, "error": error }).to_string()),
        })
    });
    Sse::new(events).into_response()
}

#[cfg(test)]
pub mod testing {
    use std::collections::BTreeMap;

    use super::*;
    use crate::backends::Context;
    use crate::jobs::job_executor::JobExecutor;
    use crate::services::atlas_service::AtlasService;
    use crate::shell::FakeShell;

    pub fn app(dir: &std::path::Path, extra: &str) -> Arc<App> {
        let config = Arc::new(
            Config::parse(&format!(
                "stage: dev\nserver: {{ bind: 127.0.0.1, port: 3001 }}\nservices: ['{0}/atlas/*.yaml']\nenvsets: {{ demo-s3: {{}} }}\ntmpDir: {0}/tmp\nbackends: {{ restic: {{ cacheDir: {0}/cache, stagingDir: {0}/staging }} }}\n{extra}",
                dir.display()
            ))
            .unwrap(),
        );
        let _ = BTreeMap::<(), ()>::new();
        let change_service = Arc::new(ChangeService::default());
        let ctx = Context {
            config: config.clone(),
            shell: Arc::new(FakeShell::default()),
        };
        let archive_service = ArchiveService::new(
            ctx,
            AtlasService::new(config.clone()),
            change_service.clone(),
        );
        let job_service = JobService::new(JobExecutor::new(archive_service.clone()).executor());
        let scheduler = Scheduler::new(archive_service.clone(), Box::new(job_service.clone()));
        Arc::new(App {
            config,
            archive_service,
            job_service,
            scheduler,
            change_service,
            sandbox: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request as HttpRequest};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    /// btrfs on two fake disks under `root`; restic's paths are the ones the fake restic answers with
    fn wiki(root: &str) -> String {
        format!(
            "service: wiki\nhome: /srv/demo/services/wiki\nbtrfs:\n  schedule: '5 * * * *'\n  subvolume: {root}/disk-a/@wiki\n  destinations: [{root}/disk-a/.snapshots, {root}/disk-b/.snapshots]\n  retention: {{ keepLast: 3, keepDaily: 7 }}\n  lifecycle: {{ stop: docker compose down, start: docker compose up --wait }}\n{RESTIC}"
        )
    }

    const RESTIC: &str = "restic:\n  repository: s3:s3.example.com/demo-backups/wiki\n  envset: demo-s3\n  retention: { keepLast: 3, keepDaily: 30, keepWeekly: 15, keepMonthly: 12 }\n  backupPaths: [/srv/demo/disk-a/@wiki/data, /srv/demo/dumps/wiki]\n  lifecycle:\n    backupPrepare: docker compose stop main\n    restoreApply: docker compose up --wait\n";

    struct Daemon {
        app: Arc<App>,
        _dir: tempfile::TempDir,
    }

    async fn daemon() -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("atlas")).unwrap();
        for disk in ["disk-a", "disk-b"] {
            std::fs::create_dir_all(root.join(disk).join(".snapshots")).unwrap();
            std::fs::write(root.join(disk).join(".fake-btrfs-filesystem"), disk).unwrap();
        }
        std::fs::create_dir_all(root.join("disk-a/@wiki")).unwrap();
        // one snapshot that never made it to disk-b
        std::fs::create_dir_all(root.join("disk-a/.snapshots/@wiki.20261001T0305")).unwrap();
        std::fs::write(root.join("atlas/wiki.yaml"), wiki(&root.to_string_lossy())).unwrap();
        let app = testing::app(dir.path(), "");
        app.archive_service.refresh().await;
        Daemon { app, _dir: dir }
    }

    async fn call(
        app: &Arc<App>,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(
                body.map(|body| Body::from(body.to_string()))
                    .unwrap_or_else(Body::empty),
            )
            .unwrap();
        let response = router(app.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(app: &Arc<App>, path: &str) -> (StatusCode, Value) {
        call(app, Method::GET, path, None).await
    }

    #[tokio::test]
    async fn should_report_its_health_and_its_build() {
        let daemon = daemon().await;
        assert_eq!(
            get(&daemon.app, "/health").await,
            (StatusCode::OK, json!({ "status": "ok" }))
        );

        let (status, env) = get(&daemon.app, "/api/env").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(env["stage"], "dev");
        assert_eq!(env["version"], build_info::VERSION);
        assert_eq!(env["commit"].as_str().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn should_answer_an_unknown_api_path_with_json_rather_than_the_website() {
        let daemon = daemon().await;
        assert_eq!(
            get(&daemon.app, "/api/nope").await,
            (StatusCode::NOT_FOUND, json!({ "error": "route_not_found" }))
        );
    }

    #[tokio::test]
    async fn should_serve_the_favicon_at_its_stable_path() {
        let daemon = daemon().await;
        let request = HttpRequest::builder()
            .uri("/favicon.svg")
            .body(Body::empty())
            .unwrap();
        let response = router(daemon.app.clone()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    }

    #[tokio::test]
    async fn should_list_the_archives_in_the_shape_the_website_reads() {
        let daemon = daemon().await;
        let (status, archives) = get(&daemon.app, "/api/archives").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(archives["refreshing"], false);
        assert_eq!(archives["errors"], json!([]));
        assert_eq!(archives["staged"], json!([]));
        let wiki = &archives["services"][0];
        assert_eq!(wiki["name"], "wiki");
        assert_eq!(wiki["backends"]["btrfs"]["state"], "ok");
        assert_eq!(wiki["backends"]["btrfs"]["info"]["backend"], "btrfs");
        assert_eq!(
            wiki["backends"]["btrfs"]["info"]["retention"],
            json!({ "keepLast": 3, "keepHourly": 0, "keepDaily": 7, "keepWeekly": 0, "keepMonthly": 0 })
        );
        assert_eq!(
            wiki["backends"]["btrfs"]["info"]["lifecycle"],
            json!({ "stop": "docker compose down", "start": "docker compose up --wait" })
        );
        assert_eq!(wiki["backends"]["restic"]["info"]["envset"], "demo-s3");
        // an absent hook is left out, not null
        assert_eq!(
            wiki["backends"]["restic"]["info"]["hooks"],
            json!({ "prepare": "docker compose stop main", "restore": "docker compose up --wait" })
        );
        assert!(wiki["backends"]["restic"].get("message").is_none());

        let btrfs = wiki["snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["backend"] == "btrfs")
            .unwrap();
        assert_eq!(
            btrfs["details"],
            json!({ "onDestinations": 1, "destinations": 2 })
        );
        assert_eq!(btrfs["handle"], "@wiki.20261001T0305");
        let restic = wiki["snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["backend"] == "restic")
            .unwrap();
        assert_eq!(
            restic["details"]["paths"],
            json!(["/srv/demo/disk-a/@wiki/data", "/srv/demo/dumps/wiki"])
        );
        assert!(restic["time"].as_str().unwrap().ends_with('Z'));

        let schedules = archives["schedules"].as_array().unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(
            (
                schedules[0]["backend"].as_str(),
                schedules[0]["cron"].as_str(),
                schedules[0]["waiting"].as_bool()
            ),
            (Some("btrfs"), Some("5 * * * *"), Some(false))
        );
    }

    #[tokio::test]
    async fn should_run_a_job_end_to_end_and_refuse_a_second_one_meanwhile() {
        let daemon = daemon().await;
        let backup = json!({ "kind": "backup", "backend": "btrfs", "targets": [{ "service": "wiki", "mode": "hot" }] });

        let (status, job) =
            call(&daemon.app, Method::POST, "/api/jobs", Some(backup.clone())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(
            (
                job["status"].as_str(),
                job["trigger"].as_str(),
                job["title"].as_str()
            ),
            (
                Some("running"),
                Some("manual"),
                Some("Backup · btrfs · wiki (hot)")
            )
        );
        let id = job["id"].as_str().unwrap();

        let (status, busy) = call(&daemon.app, Method::POST, "/api/jobs", Some(backup)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(busy, json!({ "error": "busy", "jobId": id }));

        daemon.app.job_service.settled().await;
        let (_, finished) = get(&daemon.app, &format!("/api/jobs/{id}")).await;
        assert_eq!(finished["status"], "succeeded");
        assert_eq!(finished["error"], Value::Null);
        assert_eq!(finished["failed"], json!([]));
        assert_eq!(
            finished["lines"].as_array().unwrap().last().unwrap()["text"],
            "==> Done"
        );

        let (_, jobs) = get(&daemon.app, "/api/jobs").await;
        assert_eq!(jobs[0]["id"], id);
        assert!(jobs[0].get("lines").is_none());
        let (_, archives) = get(&daemon.app, "/api/archives").await;
        let snapshots = archives["services"][0]["snapshots"].as_array().unwrap();
        let newest = snapshots.iter().find(|s| s["backend"] == "btrfs").unwrap();
        let taken = chrono::DateTime::parse_from_rfc3339(newest["time"].as_str().unwrap()).unwrap();
        assert!(
            (chrono::Utc::now() - taken.to_utc()).num_seconds() < 120,
            "newest btrfs snapshot is from {taken}"
        );
    }

    #[tokio::test]
    async fn should_fail_a_backup_that_names_an_unknown_service_before_touching_any() {
        let daemon = daemon().await;
        let backup = json!({ "kind": "backup", "backend": "btrfs", "targets": [{ "service": "wiki", "mode": "hot" }, { "service": "nope", "mode": "hot" }] });
        let (_, job) = call(&daemon.app, Method::POST, "/api/jobs", Some(backup)).await;
        daemon.app.job_service.settled().await;

        let (_, finished) = get(
            &daemon.app,
            &format!("/api/jobs/{}", job["id"].as_str().unwrap()),
        )
        .await;
        assert_eq!(finished["status"], "failed");
        assert_eq!(
            finished["error"],
            "nope is not declared by any bacre.yaml in the atlas"
        );
        assert_eq!(finished["failed"], json!([]));
        let lines: Vec<&str> = finished["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            lines,
            vec![
                "==> Refreshing the listings",
                "==> Failed: nope is not declared by any bacre.yaml in the atlas"
            ]
        );
    }

    #[tokio::test]
    async fn should_refuse_a_request_it_cannot_act_on() {
        let daemon = daemon().await;
        for body in [
            json!({ "kind": "backup", "backend": "btrfs", "targets": [] }),
            json!({ "kind": "restore", "backend": "restic", "service": "wiki", "handle": "../x" }),
            json!({ "kind": "nonsense" }),
        ] {
            let (status, answer) = call(&daemon.app, Method::POST, "/api/jobs", Some(body)).await;
            assert_eq!(
                (status, answer["error"].as_str()),
                (StatusCode::BAD_REQUEST, Some("invalid_request"))
            );
        }
        assert_eq!(
            get(&daemon.app, "/api/jobs/unknown").await,
            (StatusCode::NOT_FOUND, json!({ "error": "job_not_found" }))
        );
        assert_eq!(
            get(&daemon.app, "/api/jobs/unknown/events").await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn should_download_then_list_then_discard_a_staged_snapshot() {
        let daemon = daemon().await;
        let download = json!({ "kind": "download", "backend": "restic", "service": "wiki", "handle": "abcd1234" });
        let (status, _) = call(
            &daemon.app,
            Method::POST,
            "/api/jobs",
            Some(download.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        daemon.app.job_service.settled().await;

        let (_, archives) = get(&daemon.app, "/api/archives").await;
        let staged = &archives["staged"][0];
        assert_eq!(
            (
                staged["service"].as_str(),
                staged["backend"].as_str(),
                staged["handle"].as_str()
            ),
            (Some("wiki"), Some("restic"), Some("abcd1234"))
        );
        assert!(
            std::path::Path::new(staged["path"].as_str().unwrap())
                .join("srv/demo/dumps/wiki/wiki.sql")
                .is_file()
        );

        call(&daemon.app, Method::POST, "/api/jobs", Some(download)).await;
        daemon.app.job_service.settled().await;
        let (_, jobs) = get(&daemon.app, "/api/jobs").await;
        assert!(
            jobs.as_array()
                .unwrap()
                .iter()
                .any(|job| job["status"] == "failed"
                    && job["error"].as_str().unwrap().contains("already staged"))
        );

        assert_eq!(
            call(&daemon.app, Method::DELETE, "/api/staged/wiki/..", None)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                &daemon.app,
                Method::DELETE,
                "/api/staged/wiki/abcd1234",
                None
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        let (_, archives) = get(&daemon.app, "/api/archives").await;
        assert_eq!(archives["staged"], json!([]));
    }
}
