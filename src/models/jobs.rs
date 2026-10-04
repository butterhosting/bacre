//! A job is one backup, download or restore running on the server. Jobs live in memory
//! only: the archives are the history, a job is just the act of adding to or reading from them.
//!
//! Requests are shaped per backend, so each backend declares exactly the options it has
//! (btrfs: hot or cold; restic: none).

use serde::{Deserialize, Serialize};

use crate::models::archives::Backend;
use crate::models::atlas::is_service_name;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    Backup(BackupRequest),
    /// Fetching a snapshot into the staging directory, for the backends whose snapshots
    /// are not already on this machine. Restoring is a separate, later request, which
    /// leaves room to inspect what was downloaded.
    Download(DownloadRequest),
    /// Putting a snapshot back in place of the live data: btrfs from its snapshot, restic
    /// from its staged download
    Restore(RestoreRequest),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum BackupRequest {
    Btrfs { targets: Vec<BtrfsTarget> },
    Restic { targets: Vec<ResticTarget> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BtrfsTarget {
    pub service: String,
    /// cold stops the service around the snapshot (needs `btrfs.lifecycle` in its bacre.yaml)
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Hot,
    Cold,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Hot => "hot",
            Mode::Cold => "cold",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResticTarget {
    pub service: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub backend: DownloadBackend,
    pub service: String,
    pub handle: String,
}

/// The backends whose snapshots have to be fetched before they can be restored
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadBackend {
    Restic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub backend: Backend,
    pub service: String,
    pub handle: String,
}

impl Request {
    pub fn kind(&self) -> &'static str {
        match self {
            Request::Backup(_) => "backup",
            Request::Download(_) => "download",
            Request::Restore(_) => "restore",
        }
    }

    pub fn backend(&self) -> Backend {
        match self {
            Request::Backup(BackupRequest::Btrfs { .. }) => Backend::Btrfs,
            Request::Backup(BackupRequest::Restic { .. }) => Backend::Restic,
            Request::Download(DownloadRequest {
                backend: DownloadBackend::Restic,
                ..
            }) => Backend::Restic,
            Request::Restore(request) => request.backend,
        }
    }

    /// The services the request is about, in the order it names them
    pub fn services(&self) -> Vec<&str> {
        match self {
            Request::Backup(BackupRequest::Btrfs { targets }) => {
                targets.iter().map(|t| t.service.as_str()).collect()
            }
            Request::Backup(BackupRequest::Restic { targets }) => {
                targets.iter().map(|t| t.service.as_str()).collect()
            }
            Request::Download(request) => vec![request.service.as_str()],
            Request::Restore(request) => vec![request.service.as_str()],
        }
    }

    /// What the type system cannot say: at least one target, and names that are safe in paths
    pub fn validate(&self) -> Result<(), String> {
        let services = self.services();
        if services.is_empty() {
            return Err("targets: needs at least one".to_string());
        }
        if let Some(bad) = services.iter().find(|service| !is_service_name(service)) {
            return Err(format!("service: \"{bad}\" is not a service name"));
        }
        let handle = match self {
            Request::Backup(_) => None,
            Request::Download(request) => Some(&request.handle),
            Request::Restore(request) => Some(&request.handle),
        };
        match handle {
            Some(handle) if !is_handle(handle) => {
                Err(format!("handle: \"{handle}\" is not a snapshot handle"))
            }
            _ => Ok(()),
        }
    }

    /// What the Jobs page shows
    pub fn title(&self) -> String {
        match self {
            Request::Backup(BackupRequest::Btrfs { targets }) => {
                let targets: Vec<String> = targets
                    .iter()
                    .map(|t| format!("{} ({})", t.service, t.mode.as_str()))
                    .collect();
                format!("Backup · btrfs · {}", targets.join(", "))
            }
            Request::Backup(BackupRequest::Restic { targets }) => {
                let targets: Vec<&str> = targets.iter().map(|t| t.service.as_str()).collect();
                format!("Backup · restic · {}", targets.join(", "))
            }
            Request::Download(request) => format!(
                "Download · {} · {} · {}",
                self.backend(),
                request.service,
                request.handle
            ),
            Request::Restore(request) => format!(
                "Restore · {} · {} · {}",
                request.backend, request.service, request.handle
            ),
        }
    }
}

/// A snapshot's backend-specific identifier; it ends up in paths, hence the narrow
/// alphabet and no leading dot
pub fn is_handle(handle: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '@' || c == '_' || c == '-';
    let mut chars = handle.chars();
    chars.next().is_some_and(allowed) && chars.all(|c| allowed(c) || c == '.')
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub at: String,
    pub stream: LineStream,
    pub text: String,
}

/// `info` is Bacre narrating, `out`/`err` is what the tools and hooks printed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineStream {
    Info,
    Out,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Succeeded,
    Failed,
}

/// Who started it: someone on the website, or the scheduler
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Manual,
    Schedule,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub trigger: Trigger,
    pub request: Request,
    pub title: String,
    pub status: Status,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub error: Option<String>,
    /// The services known to have failed; a job can fail without knowing (empty)
    pub failed: Vec<String>,
    pub lines: Vec<Line>,
}

/// A job without its lines, for the list
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub trigger: Trigger,
    pub request: Request,
    pub title: String,
    pub status: Status,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub error: Option<String>,
    pub failed: Vec<String>,
}

impl From<&Job> for Summary {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            trigger: job.trigger,
            request: job.request.clone(),
            title: job.title.clone(),
            status: job.status,
            started_at: job.started_at.clone(),
            ended_at: job.ended_at.clone(),
            error: job.error.clone(),
            failed: job.failed.clone(),
        }
    }
}

/// What a job's event stream carries
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Line(Line),
    Done {
        status: Status,
        error: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn should_read_and_write_requests_in_the_shape_the_website_sends() {
        let backup = json!({ "kind": "backup", "backend": "btrfs", "targets": [{ "service": "wiki", "mode": "cold" }] });
        let request: Request = serde_json::from_value(backup.clone()).unwrap();
        assert_eq!(
            request,
            Request::Backup(BackupRequest::Btrfs {
                targets: vec![BtrfsTarget {
                    service: "wiki".into(),
                    mode: Mode::Cold
                }]
            })
        );
        assert_eq!(serde_json::to_value(&request).unwrap(), backup);
        assert_eq!(request.title(), "Backup · btrfs · wiki (cold)");

        let restore = json!({ "kind": "restore", "backend": "restic", "service": "wiki", "handle": "573591ae" });
        let request: Request = serde_json::from_value(restore.clone()).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), restore);
        assert_eq!(request.title(), "Restore · restic · wiki · 573591ae");

        let download = json!({ "kind": "download", "backend": "restic", "service": "wiki", "handle": "573591ae" });
        let request: Request = serde_json::from_value(download.clone()).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), download);
        assert_eq!(request.title(), "Download · restic · wiki · 573591ae");
    }

    #[test]
    fn should_refuse_requests_that_are_not_safe_to_act_on() {
        let parse = |value: serde_json::Value| {
            serde_json::from_value::<Request>(value)
                .map_err(|e| e.to_string())
                .and_then(|r| r.validate())
        };

        assert!(parse(json!({ "kind": "backup", "backend": "restic", "targets": [] })).is_err());
        assert!(parse(json!({ "kind": "backup", "backend": "restic", "targets": [{ "service": "../etc" }] })).is_err());
        assert!(
            parse(
                json!({ "kind": "backup", "backend": "btrfs", "targets": [{ "service": "wiki" }] })
            )
            .is_err()
        );
        assert!(
            parse(
                json!({ "kind": "download", "backend": "btrfs", "service": "wiki", "handle": "x" })
            )
            .is_err()
        );
        assert!(parse(json!({ "kind": "restore", "backend": "restic", "service": "wiki", "handle": "../../etc" })).is_err());
        assert!(parse(json!({ "kind": "restore", "backend": "restic", "service": "wiki", "handle": ".partial" })).is_err());
        assert!(parse(json!({ "kind": "restore", "backend": "btrfs", "service": "wiki", "handle": "@wiki.20261003T1505" })).is_ok());
    }
}
