//! Restic restores in two separate steps. `download` fetches a snapshot into the staging
//! directory and stops there, so it can be looked at over SSH. `restore` hands that staged
//! directory to the service's own `restoreApply` hook, which is the whole restore: Bacre
//! does not know, and does not need to know, what putting the data back involves.

use std::path::Path;

use super::{Context, hooks, restic};
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::Entry;
use crate::services::staging_service::{self, PARTIAL};

pub async fn download(ctx: &Context, entry: &Entry, handle: &str, log: &Log) -> Outcome {
    let service = &entry.config.service;
    let staged = staging_service::dir(&ctx.config, service, handle);
    if is_directory(&staged).await {
        return Err(Failure::new(format!(
            "{handle} is already staged at {}; restore it or discard it first",
            staged.display()
        )));
    }
    let partial = staged.with_file_name(format!("{handle}{PARTIAL}"));
    let _ = tokio::fs::remove_dir_all(&partial).await;
    if let Some(parent) = staged.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    log.info(format!(
        "==> Downloading {service}/{handle} into {}",
        staged.display()
    ));
    let command = restic::command(
        ctx,
        entry,
        ["restore", handle, "--target", &partial.to_string_lossy()],
    )?;
    let code = ctx
        .shell
        .stream(
            &command,
            None,
            &restic::env(ctx, entry)?,
            &|stream, text| log.line(stream, text),
        )
        .await;
    if code != 0 {
        let _ = tokio::fs::remove_dir_all(&partial).await;
        return Err(Failure::new(format!(
            "restic restore failed for {service}/{handle} (exit {code})"
        )));
    }
    tokio::fs::rename(&partial, &staged).await?;
    log.info(format!("==> Staged at {}", staged.display()));
    log.info("==> Nothing has been restored yet: inspect the directory if you like, then restore or discard it from the service page");
    Ok(())
}

pub async fn restore(ctx: &Context, entry: &Entry, handle: &str, log: &Log) -> Outcome {
    let service = &entry.config.service;
    let Some(apply) = &restic::config(entry)?.lifecycle.restore_apply else {
        return Err(Failure::new(format!(
            "{service} has no restic.lifecycle.restoreApply hook in its bacre.yaml, so Bacre does not know how to put a download back"
        )));
    };
    let staged = staging_service::dir(&ctx.config, service, handle);
    if !is_directory(&staged).await {
        return Err(Failure::new(format!(
            "{handle} is not staged (expected {}); download it first",
            staged.display()
        )));
    }
    let staged = staged.to_string_lossy();
    hooks::run(
        ctx,
        entry,
        &format!("Applying {service}/{handle} from {staged}"),
        apply,
        &[("BACRE_STAGING", &staged)],
        log,
    )
    .await?;
    log.info(format!("==> The staged download is still at {staged}; discard it from the service page when you no longer need it"));
    Ok(())
}

async fn is_directory(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_dir())
}
