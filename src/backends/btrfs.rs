use std::collections::BTreeSet;

use chrono::{NaiveDateTime, Utc};

use super::{Context, Listing};
use crate::cron;
use crate::models::archives::Backend;
use crate::models::archives::{
    BackendInfo, BtrfsRetentionInfo, LifecycleInfo, Snapshot, SnapshotDetails, iso,
};
use crate::models::atlas::{BtrfsConfig, Entry};
use crate::shell::{Env, cmd};

pub fn config(entry: &Entry) -> Result<&BtrfsConfig, String> {
    entry.config.btrfs.as_ref().ok_or_else(|| {
        format!(
            "{} has no btrfs block in its bacre.yaml",
            entry.config.service
        )
    })
}

pub fn describe(entry: &Entry) -> Result<BackendInfo, String> {
    let config = config(entry)?;
    Ok(BackendInfo::Btrfs {
        subvolume: config.subvolume.clone(),
        snapshots: config.snapshots.clone(),
        targets: config.targets.clone(),
        retention: BtrfsRetentionInfo {
            preserve_min: config.retention.preserve_min.clone(),
            preserve: config.retention.preserve.clone(),
        },
        lifecycle: config.lifecycle.as_ref().map(|lifecycle| LifecycleInfo {
            stop: lifecycle.stop.clone(),
            start: lifecycle.start.clone(),
        }),
    })
}

pub async fn list(ctx: &Context, entry: &Entry) -> Result<Listing, String> {
    let config = config(entry)?;
    let name = basename(&config.subvolume);
    let mine = |names: BTreeSet<String>| -> BTreeSet<String> {
        names
            .into_iter()
            .filter(|snapshot| {
                parse_snapshot_name(snapshot).is_some_and(|(subvolume, _)| subvolume == name)
            })
            .collect()
    };

    let live = mine(list_snapshot_names(ctx, &config.snapshots).await?);
    let mut on_target = Vec::new();
    for path in &config.targets {
        on_target.push(mine(list_snapshot_names(ctx, path).await?));
    }

    let mut snapshots: Vec<Snapshot> = live
        .into_iter()
        .filter_map(|snapshot| {
            let (_, time) = parse_snapshot_name(&snapshot)?;
            Some(Snapshot {
                backend: Backend::Btrfs,
                service: entry.config.service.clone(),
                // btrbk stamps snapshot names in local time, so the stamp is read back the same way
                time: iso(cron::local(time).with_timezone(&Utc)),
                details: SnapshotDetails::Btrfs {
                    on_targets: on_target
                        .iter()
                        .filter(|names| names.contains(&snapshot))
                        .count(),
                    targets: config.targets.len(),
                },
                handle: snapshot,
            })
        })
        .collect();
    snapshots.sort_by(|a, b| b.time.cmp(&a.time));

    Ok(Listing {
        found: !snapshots.is_empty(),
        info: describe(entry)?,
        snapshots,
    })
}

pub struct Btrbk<'a> {
    pub volume: &'a str,
    pub subvolume: &'a str,
    pub snapshot_dir: &'a str,
}

pub fn btrbk(config: &BtrfsConfig) -> Btrbk<'_> {
    let volume = dirname(&config.subvolume);
    Btrbk {
        volume,
        subvolume: basename(&config.subvolume),
        snapshot_dir: config.snapshots.get(volume.len() + 1..).unwrap_or_default(),
    }
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn dirname(path: &str) -> &str {
    &path[..path.rfind('/').unwrap_or(0)]
}

async fn list_snapshot_names(ctx: &Context, path: &str) -> Result<BTreeSet<String>, String> {
    let result = ctx
        .shell
        .run(
            &cmd(["btrfs", "subvolume", "list", "-s", path]),
            &Env::new(),
        )
        .await;
    if result.code != 0 {
        let reason = result.stderr.trim();
        let reason = if reason.is_empty() {
            format!("exit {}", result.code)
        } else {
            reason.to_string()
        };
        return Err(format!("btrfs subvolume list failed for {path}: {reason}"));
    }
    Ok(result
        .stdout
        .lines()
        .filter_map(listed_path)
        .map(|path| basename(path).to_string())
        .collect())
}

/// A line of `btrfs subvolume list`: "… otime 2026-10-01 03:05:12 path .snapshots/@wiki.20261001T0305"
fn listed_path(line: &str) -> Option<&str> {
    let line = line.trim_end();
    let (before, path) = line.rsplit_once(' ')?;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let label = before.strip_suffix("path")?;
    (!path.is_empty() && !label.chars().next_back().is_some_and(is_word)).then_some(path)
}

fn parse_snapshot_name(name: &str) -> Option<(&str, NaiveDateTime)> {
    let (subvolume, stamp) = name.split_once('.')?;
    if !subvolume.starts_with('@') || subvolume.len() < 2 || stamp.len() != 13 {
        return None;
    }
    let time = NaiveDateTime::parse_from_str(&format!("{stamp}00"), "%Y%m%dT%H%M%S").ok()?;
    Some((subvolume, time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_the_path_off_a_listing_line() {
        assert_eq!(
            listed_path(
                "ID 300 gen 3010 cgen 3003 top level 5 otime 2026-10-01 03:05:12 path .snapshots/@wiki.20261001T0305  "
            ),
            Some(".snapshots/@wiki.20261001T0305")
        );
        assert_eq!(listed_path("ID 300 top level 5 path @wiki"), Some("@wiki"));
        assert_eq!(listed_path("ID 300 top level 5 xpath @wiki"), None);
        assert_eq!(listed_path(""), None);
    }

    #[test]
    fn should_only_take_names_that_are_btrbk_snapshots() {
        assert_eq!(
            parse_snapshot_name("@wiki.20261001T0305")
                .map(|(subvolume, time)| (subvolume, time.to_string())),
            Some(("@wiki", "2026-10-01 03:05:00".to_string()))
        );
        for other in [
            "@wiki",
            "wiki.20261001T0305",
            "@.20261001T0305",
            "@wiki.20261001T03051",
            "@wiki.2026-10-01",
            "@wiki.20261001T0305.bak",
        ] {
            assert_eq!(parse_snapshot_name(other), None, "{other}");
        }
    }

    #[test]
    fn should_see_a_btrfs_block_the_way_btrbk_does() {
        let config = BtrfsConfig {
            subvolume: "/srv/disk-a/@wiki".into(),
            snapshots: "/srv/disk-a/.snapshots".into(),
            targets: vec![],
            retention: crate::models::atlas::BtrfsRetention {
                preserve_min: "24h".into(),
                preserve: vec!["30d".into()],
            },
            schedule: None,
            lifecycle: None,
        };
        let view = btrbk(&config);
        assert_eq!(
            (view.volume, view.subvolume, view.snapshot_dir),
            ("/srv/disk-a", "@wiki", ".snapshots")
        );
    }
}
