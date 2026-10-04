//! The live subvolume is moved aside first and only deleted once its replacement is in
//! place, so a failure half way never leaves the service without data.

use super::{Context, btrfs, hooks};
use crate::backends::restic::reason;
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::Entry;
use crate::shell::{Env, cmd};

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
    let snapshot = format!("{}/{handle}", config.snapshots.trim_end_matches('/'));
    let probe = ctx
        .shell
        .run(&cmd(["btrfs", "subvolume", "show", &snapshot]), &Env::new())
        .await;
    if probe.code != 0 {
        return Err(Failure::new(format!(
            "No snapshot at {snapshot}: {}",
            reason(&probe.stderr, probe.code)
        )));
    }

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
