//! Listing restic snapshots, and what the listing, the backup and the restore share.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{Context, Listing};
use crate::models::archives::{
    Backend, BackendInfo, HooksInfo, ResticRetentionInfo, Snapshot, SnapshotDetails, iso,
};
use crate::models::atlas::{Entry, ResticConfig};
use crate::shell::Env;

/// restic's documented exit code when the repository does not exist
pub const REPO_NOT_FOUND_EXIT: i32 = 10;

pub fn config(entry: &Entry) -> Result<&ResticConfig, String> {
    entry.config.restic.as_ref().ok_or_else(|| {
        format!(
            "{} has no restic block in its bacre.yaml",
            entry.config.service
        )
    })
}

/// `restic -r <repository> --cache-dir <dir>` and then whatever is asked of it
pub fn command<const N: usize>(
    ctx: &Context,
    entry: &Entry,
    args: [&str; N],
) -> Result<Vec<String>, String> {
    let cache_dir = ctx.config.backends.restic.cache_dir.to_string_lossy();
    let mut command = vec![
        "restic".to_string(),
        "-r".to_string(),
        config(entry)?.repository.clone(),
        "--cache-dir".to_string(),
        cache_dir.into_owned(),
    ];
    command.extend(args.iter().map(|arg| arg.to_string()));
    Ok(command)
}

/// The environment variables of the envset the bacre.yaml refers to (the atlas scan already checked the name exists)
pub fn env(ctx: &Context, entry: &Entry) -> Result<Env, String> {
    let name = &config(entry)?.envset;
    ctx.config.envsets.get(name).cloned().ok_or_else(|| {
        format!(
            "{} refers to the envset \"{name}\", which the daemon config does not define",
            entry.config.service
        )
    })
}

pub fn describe(entry: &Entry) -> Result<BackendInfo, String> {
    let config = config(entry)?;
    Ok(BackendInfo::Restic {
        repository: config.repository.clone(),
        envset: config.envset.clone(),
        retention: ResticRetentionInfo {
            keep_last: config.retention.keep_last,
            keep_daily: config.retention.keep_daily,
            keep_weekly: config.retention.keep_weekly,
            keep_monthly: config.retention.keep_monthly,
        },
        paths: config.backup_paths.clone(),
        hooks: HooksInfo {
            prepare: config.lifecycle.backup_prepare.clone(),
            release: config.lifecycle.backup_release.clone(),
            restore: config.lifecycle.restore_apply.clone(),
        },
    })
}

/// A snapshot as `restic snapshots --json` prints it
#[derive(Deserialize)]
struct Listed {
    time: DateTime<Utc>,
    short_id: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    paths: Option<Vec<String>>,
}

pub async fn list(ctx: &Context, entry: &Entry) -> Result<Listing, String> {
    let info = describe(entry)?;
    let result = ctx
        .shell
        .run(
            &command(ctx, entry, ["--no-lock", "--json", "snapshots"])?,
            &env(ctx, entry)?,
        )
        .await;
    if result.code == REPO_NOT_FOUND_EXIT {
        return Ok(Listing {
            found: false,
            info,
            snapshots: Vec::new(),
        });
    }
    if result.code != 0 {
        return Err(format!(
            "restic snapshots failed for {}: {}",
            config(entry)?.repository,
            reason(&result.stderr, result.code)
        ));
    }

    let listed: Vec<Listed> = if result.stdout.trim().is_empty() {
        Vec::new()
    } else {
        let repository = &config(entry)?.repository;
        serde_json::from_str(&result.stdout).map_err(|e| {
            format!("restic snapshots printed something unexpected for {repository}: {e}")
        })?
    };
    let mut snapshots: Vec<Snapshot> = listed
        .into_iter()
        .map(|snapshot| Snapshot {
            backend: Backend::Restic,
            service: entry.config.service.clone(),
            time: iso(snapshot.time),
            handle: snapshot.short_id,
            details: SnapshotDetails::Restic {
                tags: snapshot.tags.unwrap_or_default(),
                paths: snapshot.paths.unwrap_or_default(),
            },
        })
        .collect();
    snapshots.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(Listing {
        found: true,
        info,
        snapshots,
    })
}

/// What a tool said on its way out, or its exit code when it said nothing
pub fn reason(stderr: &str, code: i32) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("exit {code}")
    } else {
        stderr.to_string()
    }
}
