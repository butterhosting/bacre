//! The archives are the history: what the server reads out of the backends and what the
//! website renders. Timestamps travel as ISO strings.
//!
//! Every backend reduces to the same four facts about a snapshot: which service, when,
//! which backend, and an opaque handle only that backend can act on. Anything else a
//! backend knows goes in `details`; what it knows about the service as a whole goes in
//! `BackendInfo`.
//!
//! Adding a backend starts here: extend `Backend`, and the compiler walks you through
//! every `match` that has to learn about it.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Btrfs,
    Restic,
}

impl Backend {
    /// In the order they are shown, listed and scheduled
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

/// The form every timestamp takes on the wire: UTC, to the millisecond
pub fn iso(moment: DateTime<Utc>) -> String {
    moment.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub backend: Backend,
    pub service: String,
    pub time: String,
    /// Backend-specific identifier: the subvolume name, the restic snapshot id, …
    pub handle: String,
    pub details: SnapshotDetails,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SnapshotDetails {
    #[serde(rename_all = "camelCase")]
    Btrfs {
        /// How many of the service's targets also hold this snapshot
        on_targets: usize,
        targets: usize,
    },
    Restic {
        tags: Vec<String>,
        paths: Vec<String>,
    },
}

/// What a backend can say about a service beyond its snapshots: its configuration
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
pub enum BackendInfo {
    Btrfs {
        /// Absolute path of the live subvolume
        subvolume: String,
        /// Where its snapshots go
        snapshots: String,
        targets: Vec<String>,
        retention: BtrfsRetentionInfo,
        /// The stop and start hooks, when the service has them (cold snapshots, restores)
        lifecycle: Option<LifecycleInfo>,
    },
    Restic {
        repository: String,
        envset: String,
        retention: ResticRetentionInfo,
        paths: Vec<String>,
        hooks: HooksInfo,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtrfsRetentionInfo {
    pub preserve_min: String,
    pub preserve: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LifecycleInfo {
    pub stop: String,
    pub start: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResticRetentionInfo {
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
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
    /// The backend has nothing for this service yet (no repository, no snapshots)
    Absent,
    Error,
}

/// What a backend had to say about one service on the last refresh
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
    /// Only the backends its bacre.yaml configures
    pub backends: BTreeMap<Backend, BackendStatus>,
    /// All backends together, newest first
    pub snapshots: Vec<Snapshot>,
}

/// Something the last refresh could not do; the archives themselves are untouched.
/// Deliberately untyped beyond "where" and "what": any error anywhere fits.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    /// Where it came from: a file, a backend, a backend and service, …
    pub source: String,
    pub message: String,
}

/// A downloaded snapshot waiting in the staging directory, to be inspected, restored or discarded
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub service: String,
    pub backend: Backend,
    pub handle: String,
    pub path: String,
    pub downloaded_at: String,
}

/// A backend of a service that Bacre backs up by itself
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Schedule {
    pub service: String,
    pub backend: Backend,
    pub cron: String,
    /// Nothing for an expression that never fires
    pub next: Option<String>,
    /// Due already, and waiting for the running job to finish
    pub waiting: bool,
}

/// What the website asks for: the cached listings, and what is staged and scheduled right now
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
