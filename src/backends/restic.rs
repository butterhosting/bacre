use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{Context, Listing};
use crate::models::archives::{Backend, BackendInfo, HooksInfo, Snapshot, SnapshotDetails, iso};
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

pub fn command<const N: usize>(
    ctx: &Context,
    entry: &Entry,
    args: [&str; N],
) -> Result<Vec<String>, String> {
    let cache_dir = ctx.config.restic()?.cache_dir.to_string_lossy();
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
        retention: config.retention,
        paths: config.backup_paths.clone(),
        hooks: HooksInfo {
            prepare: config.lifecycle.backup_prepare.clone(),
            release: config.lifecycle.backup_release.clone(),
            restore: config.lifecycle.restore_apply.clone(),
        },
    })
}

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

pub fn reason(stderr: &str, code: i32) -> String {
    // with --json, restic 0.19 and later report the error itself as JSON
    let reported: Vec<String> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .filter(|value| value["message_type"] == "exit_error" || value["message_type"] == "error")
        .filter_map(|value| value["message"].as_str().map(str::to_string))
        .collect();
    if !reported.is_empty() {
        return reported.join("; ");
    }
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("exit {code}")
    } else {
        stderr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::reason;

    #[test]
    fn should_say_why_restic_failed_however_it_said_it() {
        assert_eq!(
            reason("Fatal: wrong password or no key found\n", 12),
            "Fatal: wrong password or no key found"
        );
        assert_eq!(
            reason(
                "{\"message_type\":\"exit_error\",\"code\":12,\"message\":\"Fatal: wrong password or no key found\"}\n",
                12
            ),
            "Fatal: wrong password or no key found"
        );
        assert_eq!(reason("  \n", 3), "exit 3");
    }
}
