//! Adding a backend starts here: extend `Backend`, and the compiler walks you through every
//! `match` that has to learn about it.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::retention::Retention;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Btrfs,
    Restic,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Btrfs, Backend::Restic];

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Btrfs => "btrfs",
            Backend::Restic => "restic",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn iso(moment: DateTime<Utc>) -> String {
    moment.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub backend: Backend,
    pub service: String,
    pub time: String,
    pub handle: String,
    pub details: SnapshotDetails,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SnapshotDetails {
    #[serde(rename_all = "camelCase")]
    Btrfs {
        on_snapshot_paths: usize,
        snapshot_paths: usize,
    },
    Restic {
        tags: Vec<String>,
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum BackendInfo {
    #[serde(rename_all = "camelCase")]
    Btrfs {
        subvolume: String,
        snapshot_paths: Vec<String>,
        retention: Retention,
        lifecycle: Option<LifecycleInfo>,
    },
    Restic {
        repository: String,
        envset: String,
        retention: Retention,
        paths: Vec<String>,
        hooks: HooksInfo,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LifecycleInfo {
    pub stop: String,
    pub start: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HooksInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendState {
    Ok,
    Absent,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackendStatus {
    pub state: BackendState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub info: BackendInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Service {
    pub name: String,
    pub backends: BTreeMap<Backend, BackendStatus>,
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub service: String,
    pub backend: Backend,
    pub handle: String,
    pub path: String,
    pub downloaded_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Schedule {
    pub service: String,
    pub backend: Backend,
    pub cron: String,
    pub next: Option<String>,
    pub waiting: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Archives {
    pub refreshed_at: Option<String>,
    pub refreshing: bool,
    pub errors: Vec<Problem>,
    pub services: Vec<Service>,
    pub staged: Vec<Staged>,
    pub schedules: Vec<Schedule>,
}
