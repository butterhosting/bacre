//! One module per backend and per thing it can do. The functions here are where a backend
//! is chosen: each is a `match` over `Backend`, so a new backend does not compile until it
//! has a listing, a backup and a restore.

pub mod btrfs;
pub mod btrfs_backup;
pub mod btrfs_restore;
pub mod each;
pub mod hooks;
pub mod restic;
pub mod restic_backup;
pub mod restic_restore;

use std::sync::Arc;

use crate::config::Config;
use crate::failure::Outcome;
use crate::jobs::log::Log;
use crate::models::archives::{Backend, BackendInfo, Snapshot};
use crate::models::atlas::Entry;
use crate::models::jobs::{BackupRequest, RestoreRequest};
use crate::shell::Shell;

/// What every backend works with: the daemon's configuration, and the way to its tools
#[derive(Clone)]
pub struct Context {
    pub config: Arc<Config>,
    pub shell: Arc<dyn Shell>,
}

pub struct Listing {
    /// False: nothing there yet (no repository, no snapshots)
    pub found: bool,
    pub info: BackendInfo,
    pub snapshots: Vec<Snapshot>,
}

/// What the configuration says, without touching anything (used when listing fails).
/// Only called for services whose bacre.yaml configures the backend.
pub fn describe(entry: &Entry, backend: Backend) -> Result<BackendInfo, String> {
    match backend {
        Backend::Btrfs => btrfs::describe(entry),
        Backend::Restic => restic::describe(entry),
    }
}

/// The service's snapshots in this backend, and what was found along the way
pub async fn list(ctx: &Context, entry: &Entry, backend: Backend) -> Result<Listing, String> {
    match backend {
        Backend::Btrfs => btrfs::list(ctx, entry).await,
        Backend::Restic => restic::list(ctx, entry).await,
    }
}

/// Backs up the request's targets one after the other; a failing one does not stop the rest.
/// `entry` finds the atlas entry of a target.
pub async fn backup(
    ctx: &Context,
    request: &BackupRequest,
    entry: &(dyn Fn(&str) -> Outcome<Entry> + Sync),
    log: &Log,
) -> Outcome {
    match request {
        BackupRequest::Btrfs { targets } => btrfs_backup::backup(ctx, targets, entry, log).await,
        BackupRequest::Restic { targets } => restic_backup::backup(ctx, targets, entry, log).await,
    }
}

/// Puts a snapshot back in place of a service's live data. What that means (swap a
/// subvolume, run the service's own hook) is the backend's business.
pub async fn restore(ctx: &Context, entry: &Entry, request: &RestoreRequest, log: &Log) -> Outcome {
    match request.backend {
        Backend::Btrfs => btrfs_restore::restore(ctx, entry, &request.handle, log).await,
        Backend::Restic => restic_restore::restore(ctx, entry, &request.handle, log).await,
    }
}
