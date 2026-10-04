//! The live subvolume is moved aside first and only deleted once its replacement is in
//! place, so a failure half way never leaves the service without data.

use super::{Context, btrfs, hooks};
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::{BtrfsConfig, Entry};
use crate::shell::cmd;

pub async fn restore(ctx: &Context, entry: &Entry, handle: &str, log: &Log) -> Outcome {
    let service = &entry.config.service;
    let config = btrfs::config(entry)?;
    let Some(lifecycle) = &config.lifecycle else {
        return Err(Failure::new(format!(
            "{service} has no btrfs.lifecycle block (stop and start hooks) in its bacre.yaml, so it cannot be restored"
        )));
    };
    if !handle.starts_with(&format!("{}.", btrfs::basename(&config.subvolume))) {
        return Err(Failure::new(format!(
            "{handle} is not a snapshot of {}",
            config.subvolume
        )));
    }
    let snapshot = source(ctx, config, handle).await?;

    let live = config.subvolume.as_str();
    let aside = format!("{live}.bacre-replaced");

    hooks::run(
        ctx,
        entry,
        &format!("Stopping {service}"),
        &lifecycle.stop,
        &[],
        log,
    )
    .await?;
    let swapped = swap(ctx, live, &aside, &snapshot, handle, log).await;
    // the service comes back whether or not the swap worked
    hooks::run(
        ctx,
        entry,
        &format!("Starting {service}"),
        &lifecycle.start,
        &[],
        log,
    )
    .await?;
    swapped
}

/// Only a copy on the subvolume's own filesystem can become the live subvolume in place
async fn source(ctx: &Context, config: &BtrfsConfig, handle: &str) -> Outcome<String> {
    let live = btrfs::filesystem(ctx, &config.subvolume).await?;
    let mut elsewhere = Vec::new();
    for snapshot_path in &config.snapshot_paths {
        if !btrfs::names(snapshot_path, btrfs::basename(&config.subvolume))
            .await
            .is_ok_and(|names| names.contains(handle))
        {
            continue;
        }
        if btrfs::filesystem(ctx, snapshot_path).await? == live {
            return Ok(format!("{snapshot_path}/{handle}"));
        }
        elsewhere.push(snapshot_path.as_str());
    }
    Err(Failure::new(match elsewhere.is_empty() {
        true => format!("{handle} is in none of the snapshot paths"),
        false => format!(
            "{handle} is only in {}, on another filesystem than {}; restoring from there is not supported yet",
            elsewhere.join(" and "),
            config.subvolume
        ),
    }))
}

async fn swap(
    ctx: &Context,
    live: &str,
    aside: &str,
    snapshot: &str,
    handle: &str,
    log: &Log,
) -> Outcome {
    hooks::exec(
        ctx,
        &cmd(["mv", live, aside]),
        &format!("Moving the live subvolume aside ({aside})"),
        log,
    )
    .await?;
    let restored = hooks::exec(
        ctx,
        &cmd(["btrfs", "subvolume", "snapshot", snapshot, live]),
        &format!("Restoring {handle} into {live}"),
        log,
    )
    .await;
    if let Err(failure) = restored {
        hooks::exec(
            ctx,
            &cmd(["mv", aside, live]),
            "Putting the live subvolume back",
            log,
        )
        .await?;
        return Err(failure);
    }
    hooks::exec(
        ctx,
        &cmd(["btrfs", "subvolume", "delete", aside]),
        "Deleting the replaced subvolume",
        log,
    )
    .await
}
