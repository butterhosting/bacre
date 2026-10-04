//! Each function here is a `match` over `Backend`, so a new backend does not compile until
//! it has a listing, a backup and a restore.

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

#[derive(Clone)]
pub struct Context {
    pub config: Arc<Config>,
    pub shell: Arc<dyn Shell>,
}

pub struct Listing {
    pub found: bool,
    pub info: BackendInfo,
    pub snapshots: Vec<Snapshot>,
}

pub fn describe(entry: &Entry, backend: Backend) -> Result<BackendInfo, String> {
    match backend {
        Backend::Btrfs => btrfs::describe(entry),
        Backend::Restic => restic::describe(entry),
    }
}

pub async fn list(ctx: &Context, entry: &Entry, backend: Backend) -> Result<Listing, String> {
    match backend {
        Backend::Btrfs => btrfs::list(ctx, entry).await,
        Backend::Restic => restic::list(ctx, entry).await,
    }
}

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

pub async fn restore(ctx: &Context, entry: &Entry, request: &RestoreRequest, log: &Log) -> Outcome {
    match request.backend {
        Backend::Btrfs => btrfs_restore::restore(ctx, entry, &request.handle, log).await,
        Backend::Restic => restic_restore::restore(ctx, entry, &request.handle, log).await,
    }
}
