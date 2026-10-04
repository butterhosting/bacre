//! Tells the configured webhook how every job ended: one POST per job, the same shape for
//! all of them. Delivery is best effort (a few attempts, then a line in the daemon's log):
//! the job's own outcome never depends on it.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::Sha256;

use crate::config::Config;
use crate::models::archives::Backend;
use crate::models::jobs::{BackupRequest, Job, Request, Status, Trigger};

const ATTEMPTS: u32 = 3;
const TIMEOUT: Duration = Duration::from_secs(10);
/// How much of the end of the job's log goes along
const TAIL: usize = 15;

/// The one thing the service needs from the network
#[async_trait]
pub trait Poster: Send + Sync {
    /// The status code of the answer, or why there was none
    async fn post(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
        body: String,
    ) -> Result<u16, String>;
}

pub struct HttpPoster(reqwest::Client);

impl Default for HttpPoster {
    fn default() -> Self {
        Self(
            reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .expect("an http client"),
        )
    }
}

#[async_trait]
impl Poster for HttpPoster {
    async fn post(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
        body: String,
    ) -> Result<u16, String> {
        let mut request = self.0.post(url).body(body);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        request
            .send()
            .await
            .map(|response| response.status().as_u16())
            .map_err(|e| e.to_string())
    }
}

pub struct WebhookService {
    config: Arc<Config>,
    poster: Box<dyn Poster>,
    /// Between attempts: this long after the first, twice this after the second
    pause: Duration,
}

impl WebhookService {
    pub fn new(config: Arc<Config>) -> Self {
        Self::with(
            config,
            Box::new(HttpPoster::default()),
            Duration::from_secs(2),
        )
    }

    pub fn with(config: Arc<Config>, poster: Box<dyn Poster>, pause: Duration) -> Self {
        Self {
            config,
            poster,
            pause,
        }
    }

    pub async fn notify(&self, job: &Job) {
        let Some(webhook) = &self.config.webhook else {
            return;
        };
        let Some(payload) = payload(job) else { return };
        let body = serde_json::to_string(&payload).expect("a payload is always serializable");

        let mut headers = vec![
            ("content-type", "application/json".to_string()),
            ("user-agent", "Bacre".to_string()),
            ("x-bacre-event", payload.event.to_string()),
        ];
        if let Some(secret) = &webhook.secret {
            // bare hex, the way Forgejo signs its webhooks
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .expect("hmac takes a key of any length");
            mac.update(body.as_bytes());
            headers.push((
                "x-bacre-signature",
                hex::encode(mac.finalize().into_bytes()),
            ));
        }

        let mut problem = String::new();
        for attempt in 1..=ATTEMPTS {
            match self.poster.post(&webhook.url, &headers, body.clone()).await {
                Ok(status) if (200..300).contains(&status) => return,
                Ok(status) => problem = format!("HTTP {status}"),
                Err(e) => problem = e,
            }
            if attempt < ATTEMPTS {
                tokio::time::sleep(self.pause * attempt).await;
            }
        }
        eprintln!(
            "==> Webhook: could not report job {} ({}): {problem}",
            job.id, payload.event
        );
    }
}

#[derive(Debug, Serialize)]
pub struct Payload {
    /// `job_succeeded` or `job_failed`: the same word as the `X-Bacre-Event` header
    pub event: &'static str,
    pub host: String,
    pub job: JobPayload,
}

#[derive(Debug, Serialize)]
pub struct JobPayload {
    pub id: String,
    pub kind: &'static str,
    pub backend: Backend,
    pub services: Vec<ServicePayload>,
    pub trigger: Trigger,
    pub status: Status,
    /// Why it failed; null when it did not
    pub error: Option<String>,
    pub started: String,
    pub completed: String,
    /// The last lines of the job's log, newline-separated
    pub tail: String,
}

/// A service the job was about, with whatever this kind of job and backend had to say about it
#[derive(Debug, Serialize)]
pub struct ServicePayload {
    pub name: String,
    /// Whether this service is why the job failed. A job can fail before reaching any service: then none is marked
    pub failed: bool,
    /// btrfs backups: hot or cold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
    /// Downloads and restores: the snapshot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

/// Nothing for a job that is still running
pub fn payload(job: &Job) -> Option<Payload> {
    let event = match job.status {
        Status::Running => return None,
        Status::Succeeded => "job_succeeded",
        Status::Failed => "job_failed",
    };
    let tail_from = job.lines.len().saturating_sub(TAIL);
    Some(Payload {
        event,
        host: gethostname::gethostname().to_string_lossy().into_owned(),
        job: JobPayload {
            id: job.id.clone(),
            kind: job.request.kind(),
            backend: job.request.backend(),
            services: services(job),
            trigger: job.trigger,
            status: job.status,
            error: job.error.clone(),
            started: job.started_at.clone(),
            completed: job
                .ended_at
                .clone()
                .unwrap_or_else(|| job.started_at.clone()),
            tail: job.lines[tail_from..]
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        },
    })
}

fn services(job: &Job) -> Vec<ServicePayload> {
    let failed = |service: &str| job.failed.iter().any(|failed| failed == service);
    // one service, so its failure is the job's
    let single = |service: &str, handle: &str| {
        vec![ServicePayload {
            name: service.to_string(),
            failed: job.status == Status::Failed,
            mode: None,
            snapshot: Some(handle.to_string()),
        }]
    };
    match &job.request {
        Request::Backup(BackupRequest::Btrfs { targets }) => targets
            .iter()
            .map(|target| ServicePayload {
                name: target.service.clone(),
                failed: failed(&target.service),
                mode: Some(target.mode.as_str()),
                snapshot: None,
            })
            .collect(),
        Request::Backup(BackupRequest::Restic { targets }) => targets
            .iter()
            .map(|target| ServicePayload {
                name: target.service.clone(),
                failed: failed(&target.service),
                mode: None,
                snapshot: None,
            })
            .collect(),
        Request::Download(request) => single(&request.service, &request.handle),
        Request::Restore(request) => single(&request.service, &request.handle),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::{Value, json};

    use super::*;
    use crate::config::testing;
    use crate::models::jobs::{Line, LineStream, ResticTarget, RestoreRequest};

    fn job() -> Job {
        let request = Request::Backup(BackupRequest::Restic {
            targets: vec![
                ResticTarget {
                    service: "wiki".into(),
                },
                ResticTarget {
                    service: "mailbox".into(),
                },
            ],
        });
        Job {
            id: "a1b2c3d4".into(),
            trigger: Trigger::Schedule,
            title: request.title(),
            request,
            status: Status::Failed,
            started_at: "2026-10-03T01:00:00.000Z".into(),
            ended_at: Some("2026-10-03T01:00:41.000Z".into()),
            error: Some("1 of 2 failed: mailbox".into()),
            failed: vec!["mailbox".into()],
            lines: vec![
                Line {
                    at: "2026-10-03T01:00:40.000Z".into(),
                    stream: LineStream::Err,
                    text: "Fatal: unable to open repository".into(),
                },
                Line {
                    at: "2026-10-03T01:00:41.000Z".into(),
                    stream: LineStream::Info,
                    text: "==> Failed: 1 of 2 failed: mailbox".into(),
                },
            ],
        }
    }

    struct Call {
        url: String,
        headers: Vec<(&'static str, String)>,
        body: String,
    }

    /// A receiver that answers with the given statuses in turn and remembers what it was sent
    #[derive(Clone, Default)]
    struct Receiver {
        statuses: Vec<u16>,
        calls: Arc<Mutex<Vec<Call>>>,
    }

    #[async_trait]
    impl Poster for Receiver {
        async fn post(
            &self,
            url: &str,
            headers: &[(&'static str, String)],
            body: String,
        ) -> Result<u16, String> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(Call {
                url: url.to_string(),
                headers: headers.to_vec(),
                body,
            });
            Ok(self.statuses.get(calls.len() - 1).copied().unwrap_or(200))
        }
    }

    fn service(webhook: &str, statuses: &[u16]) -> (WebhookService, Arc<Mutex<Vec<Call>>>) {
        let receiver = Receiver {
            statuses: statuses.to_vec(),
            ..Receiver::default()
        };
        let calls = receiver.calls.clone();
        (
            WebhookService::with(
                Arc::new(testing::config(webhook)),
                Box::new(receiver),
                Duration::ZERO,
            ),
            calls,
        )
    }

    fn header<'a>(call: &'a Call, name: &str) -> Option<&'a str> {
        call.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    #[tokio::test]
    async fn should_post_the_job_with_its_event_in_a_header_and_a_signature_over_the_body() {
        let (service, calls) = service(
            "webhook: { url: 'http://hooks/bacre', secret: s3cret }",
            &[200],
        );
        service.notify(&job()).await;

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, "http://hooks/bacre");
        assert_eq!(header(&calls[0], "x-bacre-event"), Some("job_failed"));
        let mut mac = Hmac::<Sha256>::new_from_slice(b"s3cret").unwrap();
        mac.update(calls[0].body.as_bytes());
        assert_eq!(
            header(&calls[0], "x-bacre-signature"),
            Some(hex::encode(mac.finalize().into_bytes()).as_str())
        );

        let body: Value = serde_json::from_str(&calls[0].body).unwrap();
        assert_eq!(body["event"], "job_failed");
        assert_eq!(
            body["job"],
            json!({
                "id": "a1b2c3d4",
                "kind": "backup",
                "backend": "restic",
                "services": [
                    { "name": "wiki", "failed": false },
                    { "name": "mailbox", "failed": true },
                ],
                "trigger": "schedule",
                "status": "failed",
                "error": "1 of 2 failed: mailbox",
                "started": "2026-10-03T01:00:00.000Z",
                "completed": "2026-10-03T01:00:41.000Z",
                "tail": "Fatal: unable to open repository\n==> Failed: 1 of 2 failed: mailbox",
            })
        );
    }

    #[tokio::test]
    async fn should_not_sign_without_a_secret_and_name_the_one_service_of_a_restore() {
        let (service, calls) = service("webhook: { url: 'http://hooks/bacre' }", &[200]);
        let restore = Job {
            status: Status::Succeeded,
            error: None,
            failed: vec![],
            request: Request::Restore(RestoreRequest {
                backend: Backend::Btrfs,
                service: "wiki".into(),
                handle: "@wiki.20261003T0300".into(),
            }),
            ..job()
        };
        service.notify(&restore).await;

        let calls = calls.lock().unwrap();
        assert_eq!(header(&calls[0], "x-bacre-signature"), None);
        assert_eq!(header(&calls[0], "x-bacre-event"), Some("job_succeeded"));
        let body: Value = serde_json::from_str(&calls[0].body).unwrap();
        assert_eq!(
            body["job"]["services"],
            json!([{ "name": "wiki", "failed": false, "snapshot": "@wiki.20261003T0300" }])
        );
        assert_eq!(body["job"]["error"], Value::Null);
    }

    #[tokio::test]
    async fn should_try_again_when_the_receiver_fails_and_give_up_quietly_after_three_attempts() {
        let (flaky, flaky_calls) = service("webhook: { url: 'http://hooks/bacre' }", &[500, 200]);
        flaky.notify(&job()).await;
        assert_eq!(flaky_calls.lock().unwrap().len(), 2);

        let (down, down_calls) = service(
            "webhook: { url: 'http://hooks/bacre' }",
            &[500, 500, 500, 200],
        );
        down.notify(&job()).await;
        assert_eq!(down_calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn should_do_nothing_without_a_webhook_or_for_a_job_that_still_runs() {
        let (silent, silent_calls) = service("", &[]);
        silent.notify(&job()).await;
        assert_eq!(silent_calls.lock().unwrap().len(), 0);

        let (service, calls) = service("webhook: { url: 'http://hooks/bacre' }", &[]);
        service
            .notify(&Job {
                status: Status::Running,
                ..job()
            })
            .await;
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn should_only_send_the_end_of_a_long_log() {
        let line = |n: usize| Line {
            at: "2026-10-03T01:00:40.000Z".into(),
            stream: LineStream::Out,
            text: format!("line {n}"),
        };
        let long = Job {
            lines: (1..=40).map(line).collect(),
            ..job()
        };
        let tail = payload(&long).unwrap().job.tail;
        assert_eq!(tail.lines().count(), 15);
        assert!(tail.starts_with("line 26\n") && tail.ends_with("line 40"));
    }
}
