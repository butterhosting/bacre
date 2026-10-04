use super::each::each;
use super::{Context, btrfs, hooks};
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::{BtrfsConfig, Entry};
use crate::models::jobs::{BtrfsTarget, Mode};
use crate::shell::{Env, cmd};

pub async fn backup(
    ctx: &Context,
    targets: &[BtrfsTarget],
    entry: &(dyn Fn(&str) -> Outcome<Entry> + Sync),
    log: &Log,
) -> Outcome {
    each(targets, |target| &target.service, log, |target| async move {
        let service = &target.service;
        let entry = entry(service)?;
        let config = btrfs::config(&entry)?;
        let lifecycle = match (target.mode, &config.lifecycle) {
            (Mode::Hot, _) => None,
            (Mode::Cold, Some(lifecycle)) => Some(lifecycle),
            (Mode::Cold, None) => {
                return Err(Failure::new(format!(
                    "{service} has no btrfs.lifecycle block (stop and start hooks) in its bacre.yaml, so it cannot be snapshotted cold"
                )));
            }
        };

        if let Some(lifecycle) = lifecycle {
            hooks::run(ctx, &entry, &format!("Stopping {service}"), &lifecycle.stop, &[], log).await?;
        }
        let snapshotted = btrbk(ctx, &entry, config, log).await;
        // the service comes back whether or not the snapshot worked
        if let Some(lifecycle) = lifecycle {
            hooks::run(ctx, &entry, &format!("Starting {service}"), &lifecycle.start, &[], log).await?;
        }
        snapshotted
    })
    .await
}

async fn btrbk(ctx: &Context, entry: &Entry, config: &BtrfsConfig, log: &Log) -> Outcome {
    let service = &entry.config.service;
    let view = btrfs::btrbk(config);
    tokio::fs::create_dir_all(&ctx.config.tmp_dir).await?;
    let config_path = ctx
        .config
        .tmp_dir
        .join(format!("bacre-btrbk-{}.conf", uuid::Uuid::new_v4()));
    tokio::fs::write(&config_path, render(config)).await?;

    log.info(format!(
        "==> Snapshotting {service} ({}) with btrbk",
        config.subvolume
    ));
    let command = cmd([
        "btrbk",
        "-c",
        &config_path.to_string_lossy(),
        "run",
        view.subvolume,
    ]);
    let code = ctx
        .shell
        .stream(&command, None, &Env::new(), &|stream, text| {
            log.line(stream, text)
        })
        .await;
    let _ = tokio::fs::remove_file(&config_path).await;

    if code != 0 {
        return Err(Failure::new(format!(
            "btrbk failed for {service} (exit {code})"
        )));
    }
    Ok(())
}

fn render(config: &BtrfsConfig) -> String {
    let view = btrfs::btrbk(config);
    let preserve_min = &config.retention.preserve_min;
    let preserve = config.retention.preserve.join(" ");
    let mut lines = vec![
        format!("snapshot_dir {}", view.snapshot_dir),
        "timestamp_format long".to_string(),
        String::new(),
        format!("volume {}", view.volume),
        format!("  subvolume {}", view.subvolume),
        format!("    snapshot_preserve_min {preserve_min}"),
        format!("    snapshot_preserve {preserve}"),
    ];
    for target in &config.targets {
        lines.push(format!("    target {target}"));
        lines.push(format!("      target_preserve_min {preserve_min}"));
        lines.push(format!("      target_preserve {preserve}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::atlas::BtrfsRetention;

    #[test]
    fn should_render_the_btrbk_configuration_of_one_subvolume() {
        let config = BtrfsConfig {
            subvolume: "/srv/disk-a/@wiki".into(),
            snapshots: "/srv/disk-a/.snapshots".into(),
            targets: vec!["/srv/disk-b/.snapshots".into()],
            retention: BtrfsRetention {
                preserve_min: "24h".into(),
                preserve: vec!["72h".into(), "30d".into()],
            },
            schedule: None,
            lifecycle: None,
        };
        assert_eq!(
            render(&config),
            "snapshot_dir .snapshots\ntimestamp_format long\n\nvolume /srv/disk-a\n  subvolume @wiki\n    snapshot_preserve_min 24h\n    snapshot_preserve 72h 30d\n    target /srv/disk-b/.snapshots\n      target_preserve_min 24h\n      target_preserve 72h 30d\n"
        );
    }
}
