use std::collections::BTreeSet;

use chrono::{NaiveDateTime, Utc};

use super::{Context, Listing};
use crate::cron;
use crate::models::archives::{
    Backend, BackendInfo, LifecycleInfo, Snapshot, SnapshotDetails, iso,
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
        destinations: config.destinations.clone(),
        retention: config.retention,
        lifecycle: config.lifecycle.as_ref().map(|lifecycle| LifecycleInfo {
            stop: lifecycle.stop.clone(),
            start: lifecycle.start.clone(),
        }),
    })
}

pub async fn list(_ctx: &Context, entry: &Entry) -> Result<Listing, String> {
    let config = config(entry)?;
    let name = basename(&config.subvolume);
    let mut found = Vec::new();
    for destination in &config.destinations {
        found.push(names(destination, name).await?);
    }
    let all: BTreeSet<&String> = found.iter().flatten().collect();

    let mut snapshots: Vec<Snapshot> = all
        .into_iter()
        .filter_map(|snapshot| {
            let (_, time) = parse_snapshot_name(snapshot)?;
            Some(Snapshot {
                backend: Backend::Btrfs,
                service: entry.config.service.clone(),
                time: iso(cron::local(time).with_timezone(&Utc)),
                details: SnapshotDetails::Btrfs {
                    on_destinations: found
                        .iter()
                        .filter(|names| names.contains(snapshot))
                        .count(),
                    destinations: config.destinations.len(),
                },
                handle: snapshot.clone(),
            })
        })
        .collect();
    snapshots.sort_by(|a, b| b.time.cmp(&a.time).then(b.handle.cmp(&a.handle)));

    Ok(Listing {
        found: !snapshots.is_empty(),
        info: describe(entry)?,
        snapshots,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snap {
    pub name: String,
    pub time: NaiveDateTime,
    pub uuid: String,
    pub received: Option<String>,
    pub read_only: bool,
}

/// The snapshots of the subvolume called `name` in `dir`, oldest first
pub async fn names(dir: &str, name: &str) -> Result<BTreeSet<String>, String> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("cannot read {dir}: {e}"))?;
    let mut names = BTreeSet::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("cannot read {dir}: {e}"))?
    {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if parse_snapshot_name(&file_name).is_some_and(|(subvolume, _)| subvolume == name)
            && entry.path().is_dir()
        {
            names.insert(file_name);
        }
    }
    Ok(names)
}

/// What btrfs knows of each snapshot in `dir`, oldest first
pub async fn inventory(ctx: &Context, dir: &str, name: &str) -> Result<Vec<Snap>, String> {
    let mut snaps = Vec::new();
    for snapshot in names(dir, name).await? {
        let path = format!("{dir}/{snapshot}");
        let output = ctx
            .shell
            .run(&cmd(["btrfs", "subvolume", "show", &path]), &Env::new())
            .await;
        if output.code != 0 {
            return Err(format!(
                "btrfs subvolume show failed for {path}: {}",
                output.stderr.trim()
            ));
        }
        let (_, time) =
            parse_snapshot_name(&snapshot).expect("names() only returns snapshot names");
        let shown = |key: &str| {
            output.stdout.lines().find_map(|line| {
                let (k, v) = line.split_once(':')?;
                (k.trim() == key).then(|| v.trim().to_string())
            })
        };
        let uuid = shown("UUID")
            .filter(|uuid| uuid != "-")
            .ok_or_else(|| format!("btrfs subvolume show gave no UUID for {path}"))?;
        snaps.push(Snap {
            uuid,
            received: shown("Received UUID").filter(|uuid| uuid != "-"),
            read_only: shown("Flags")
                .is_some_and(|flags| flags.split(',').any(|flag| flag.trim() == "readonly")),
            name: snapshot,
            time,
        });
    }
    snaps.sort_by(|a, b| a.time.cmp(&b.time).then(a.name.cmp(&b.name)));
    Ok(snaps)
}

/// The UUID of the filesystem `path` is on; the device number would not do, since every
/// subvolume gets one of its own
pub async fn filesystem(ctx: &Context, path: &str) -> Result<String, String> {
    let output = ctx
        .shell
        .run(
            &cmd([
                "findmnt",
                "--noheadings",
                "--output",
                "UUID",
                "--target",
                path,
            ]),
            &Env::new(),
        )
        .await;
    let uuid = output.stdout.trim();
    if output.code != 0 || uuid.is_empty() {
        let reason = output.stderr.trim();
        return Err(format!(
            "cannot tell which filesystem {path} is on{}",
            if reason.is_empty() {
                String::new()
            } else {
                format!(": {reason}")
            }
        ));
    }
    Ok(uuid.to_string())
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `@wiki.20261001T0305`, stamped on the local clock, with `_1`, `_2`, … for more in the same minute
pub fn parse_snapshot_name(name: &str) -> Option<(&str, NaiveDateTime)> {
    let (subvolume, stamp) = name.split_once('.')?;
    let stamp = match stamp.split_once('_') {
        Some((stamp, nr)) if !nr.is_empty() && nr.chars().all(|c| c.is_ascii_digit()) => stamp,
        Some(_) => return None,
        None => stamp,
    };
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
    fn should_only_take_names_that_are_snapshots() {
        let parsed =
            |name| parse_snapshot_name(name).map(|(subvolume, time)| (subvolume, time.to_string()));
        assert_eq!(
            parsed("@wiki.20261001T0305"),
            Some(("@wiki", "2026-10-01 03:05:00".to_string()))
        );
        assert_eq!(
            parsed("@wiki.20261001T0305_2"),
            Some(("@wiki", "2026-10-01 03:05:00".to_string()))
        );
        for other in [
            "@wiki",
            "wiki.20261001T0305",
            "@.20261001T0305",
            "@wiki.20261001T03051",
            "@wiki.2026-10-01",
            "@wiki.20261001T0305.bak",
            "@wiki.20261001T0305_",
            "@wiki.20261001T0305_x",
        ] {
            assert_eq!(parse_snapshot_name(other), None, "{other}");
        }
    }
}
