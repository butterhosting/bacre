use chrono::Local;

use super::each::each;
use super::restic::{REPO_NOT_FOUND_EXIT, reason};
use super::{Context, hooks, restic};
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::{Entry, ResticConfig};
use crate::models::jobs::ResticTarget;

pub async fn backup(
    ctx: &Context,
    targets: &[ResticTarget],
    entry: &(dyn Fn(&str) -> Outcome<Entry> + Sync),
    log: &Log,
) -> Outcome {
    each(
        targets,
        |target| &target.service,
        log,
        |target| async move {
            let service = &target.service;
            let entry = entry(service)?;
            let config = restic::config(&entry)?;

            ensure_repository(ctx, &entry, log).await?;
            if let Some(prepare) = &config.lifecycle.backup_prepare {
                hooks::run(
                    ctx,
                    &entry,
                    &format!("Preparing {service}"),
                    prepare,
                    &[],
                    log,
                )
                .await?;
            }
            let backed_up = backup_and_prune(ctx, &entry, config, log).await;
            // what `backupPrepare` did is undone whether or not the backup worked
            if let Some(release) = &config.lifecycle.backup_release {
                hooks::run(
                    ctx,
                    &entry,
                    &format!("Releasing {service}"),
                    release,
                    &[],
                    log,
                )
                .await?;
            }
            backed_up
        },
    )
    .await
}

async fn backup_and_prune(
    ctx: &Context,
    entry: &Entry,
    config: &ResticConfig,
    log: &Log,
) -> Outcome {
    let service = &entry.config.service;
    let tag = format!("t={}", Local::now().format("%Y%m%dT%H%M"));
    let mut backup = restic::command(ctx, entry, ["backup", "--tag", &tag])?;
    backup.extend(config.backup_paths.iter().cloned());
    stream(
        ctx,
        entry,
        &backup,
        &format!("Backing up {service} ({} paths)", config.backup_paths.len()),
        log,
    )
    .await?;

    let keep = &config.retention;
    // one policy over the whole repository (a repository is one service): no grouping, so snapshots taken
    // under another hostname or with other paths thin out with the rest instead of being kept forever
    let forget = restic::command(
        ctx,
        entry,
        [
            "forget",
            "--group-by",
            "",
            "--keep-last",
            &keep.keep_last.to_string(),
            "--keep-hourly",
            &keep.keep_hourly.to_string(),
            "--keep-daily",
            &keep.keep_daily.to_string(),
            "--keep-weekly",
            &keep.keep_weekly.to_string(),
            "--keep-monthly",
            &keep.keep_monthly.to_string(),
            "--prune",
        ],
    )?;
    stream(
        ctx,
        entry,
        &forget,
        &format!("Applying retention for {service}"),
        log,
    )
    .await
}

async fn ensure_repository(ctx: &Context, entry: &Entry, log: &Log) -> Outcome {
    let service = &entry.config.service;
    let env = restic::env(ctx, entry)?;
    let probe = ctx
        .shell
        .run(
            &restic::command(ctx, entry, ["--no-lock", "cat", "config"])?,
            &env,
        )
        .await;
    if probe.code == 0 {
        return Ok(());
    }
    if probe.code != REPO_NOT_FOUND_EXIT {
        return Err(Failure::new(format!(
            "restic could not open the repository for {service}: {}",
            reason(&probe.stderr, probe.code)
        )));
    }
    log.info(format!(
        "==> No repository yet for {service}, initialising it"
    ));
    let init = ctx
        .shell
        .run(&restic::command(ctx, entry, ["init"])?, &env)
        .await;
    if init.code != 0 {
        return Err(Failure::new(format!(
            "restic init failed for {service}: {}",
            reason(&init.stderr, init.code)
        )));
    }
    Ok(())
}

pub async fn stream(
    ctx: &Context,
    entry: &Entry,
    command: &[String],
    label: &str,
    log: &Log,
) -> Outcome {
    log.info(format!("==> {label}"));
    let code = ctx
        .shell
        .stream(command, None, &restic::env(ctx, entry)?, &|stream, text| {
            log.line(stream, text)
        })
        .await;
    if code != 0 {
        return Err(Failure::new(format!("{label} failed (exit {code})")));
    }
    Ok(())
}
