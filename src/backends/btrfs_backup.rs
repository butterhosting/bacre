//! A snapshot is taken into the one snapshot path on the subvolume's filesystem, then sent on to
//! the others. A send is incremental against the newest snapshot both sides have; a copy only
//! counts as "had" when btrfs recorded it as received from that very snapshot, and retention
//! never deletes that pair, so the next send has its parent.

use std::collections::BTreeSet;

use chrono::{Local, NaiveDateTime};

use super::btrfs::{self, Snap};
use super::each::each;
use super::{Context, hooks};
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::{BtrfsConfig, BtrfsLifecycle, Entry};
use crate::models::jobs::{BtrfsTarget, Mode};
use crate::retention::Retention;
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
        snapshot(ctx, &entry, config, lifecycle, Local::now().naive_local(), log).await
    })
    .await
}

struct Layout {
    local: String,
    others: Vec<String>,
}

async fn snapshot(
    ctx: &Context,
    entry: &Entry,
    config: &BtrfsConfig,
    lifecycle: Option<&BtrfsLifecycle>,
    now: NaiveDateTime,
    log: &Log,
) -> Outcome {
    let service = &entry.config.service;
    let name = btrfs::basename(&config.subvolume);
    let mut failures = Vec::new();
    let (layout, unreachable) = layout(ctx, config).await?;
    for (snapshot_path, reason) in unreachable {
        log.err(format!("Skipping {snapshot_path}: {reason}"));
        failures.push(reason);
    }

    let mut others = Vec::new();
    for snapshot_path in &layout.others {
        match clean(ctx, snapshot_path, name, log).await {
            Ok(()) => others.push(snapshot_path.as_str()),
            Err(reason) => {
                log.err(format!("Skipping {snapshot_path}: {reason}"));
                failures.push(reason);
            }
        }
    }

    let mut taken = btrfs::names(&layout.local, name).await?;
    for snapshot_path in &others {
        taken.extend(btrfs::names(snapshot_path, name).await?);
    }
    let new_name = next_name(name, now, &taken);
    let new_path = format!("{}/{new_name}", layout.local);
    if let Some(lifecycle) = lifecycle {
        hooks::run(
            ctx,
            entry,
            &format!("Stopping {service}"),
            &lifecycle.stop,
            &[],
            log,
        )
        .await?;
    }
    let created = hooks::exec(
        ctx,
        &cmd([
            "btrfs",
            "subvolume",
            "snapshot",
            "-r",
            &config.subvolume,
            &new_path,
        ]),
        &format!("Snapshotting {} into {}", config.subvolume, layout.local),
        log,
    )
    .await;
    // the service comes back whether or not the snapshot worked
    if let Some(lifecycle) = lifecycle {
        hooks::run(
            ctx,
            entry,
            &format!("Starting {service}"),
            &lifecycle.start,
            &[],
            log,
        )
        .await?;
    }
    created?;

    let local = btrfs::inventory(ctx, &layout.local, name).await?;
    let new = local
        .iter()
        .find(|snap| snap.name == new_name)
        .ok_or_else(|| format!("{new_path} is missing right after it was made"))?;
    for snapshot_path in &others {
        let sent = match btrfs::inventory(ctx, snapshot_path, name).await {
            Ok(remote) => {
                send(
                    ctx,
                    &layout.local,
                    snapshot_path,
                    new,
                    parent(&local, &remote),
                    log,
                )
                .await
            }
            Err(reason) => Err(reason),
        };
        if let Err(reason) = sent {
            log.err(&reason);
            failures.push(reason);
            // a receive that stopped half way leaves a copy that must not count as one
            let _ = clean(ctx, snapshot_path, name, log).await;
        }
    }

    let mut protected = BTreeSet::new();
    let mut remotes = Vec::new();
    for snapshot_path in &others {
        match btrfs::inventory(ctx, snapshot_path, name).await {
            Ok(remote) => {
                let kept = parent(&local, &remote).map(|snap| snap.name.clone());
                protected.extend(kept.clone());
                remotes.push((snapshot_path, remote, kept));
            }
            Err(reason) => failures.push(reason),
        }
    }
    if let Err(reason) = prune(
        ctx,
        &layout.local,
        &local,
        &config.retention,
        &protected,
        log,
    )
    .await
    {
        failures.push(reason);
    }
    for (snapshot_path, remote, kept) in remotes {
        if let Err(reason) = prune(
            ctx,
            snapshot_path,
            &remote,
            &config.retention,
            &kept.into_iter().collect(),
            log,
        )
        .await
        {
            failures.push(reason);
        }
    }

    match failures.is_empty() {
        true => Ok(()),
        false => Err(Failure::new(failures.join("; "))),
    }
}

/// Which snapshot path is on the subvolume's filesystem, and which cannot be reached at all
async fn layout(ctx: &Context, config: &BtrfsConfig) -> Outcome<(Layout, Vec<(String, String)>)> {
    let live = btrfs::filesystem(ctx, &config.subvolume).await?;
    let (mut local, mut others, mut unreachable) = (Vec::new(), Vec::new(), Vec::new());
    for snapshot_path in &config.snapshot_paths {
        let found = match tokio::fs::metadata(snapshot_path).await {
            Ok(meta) if meta.is_dir() => btrfs::filesystem(ctx, snapshot_path).await,
            _ => Err(format!("{snapshot_path} is not there")),
        };
        match found {
            Ok(uuid) if uuid == live => local.push(snapshot_path.clone()),
            Ok(_) => others.push(snapshot_path.clone()),
            Err(reason) => unreachable.push((snapshot_path.clone(), reason)),
        }
    }
    match local.len() {
        1 => Ok((
            Layout {
                local: local.remove(0),
                others,
            },
            unreachable,
        )),
        0 => Err(Failure::new(format!(
            "none of the snapshot paths is on the filesystem of {}, so there is nowhere to take a snapshot{}",
            config.subvolume,
            unreachable
                .iter()
                .map(|(_, reason)| format!("; {reason}"))
                .collect::<String>()
        ))),
        _ => Err(Failure::new(format!(
            "{} are all on the filesystem of {}; only one snapshot path may be",
            local.join(" and "),
            config.subvolume
        ))),
    }
}

async fn send(
    ctx: &Context,
    from: &str,
    to: &str,
    snap: &Snap,
    parent: Option<&Snap>,
    log: &Log,
) -> Result<(), String> {
    let mut send = cmd(["btrfs", "send"]);
    match parent {
        Some(parent) => {
            log.info(format!(
                "==> Sending {} to {to}, as the changes since {}",
                snap.name, parent.name
            ));
            send.extend(["-p".to_string(), format!("{from}/{}", parent.name)]);
        }
        None => log.info(format!(
            "==> Sending all of {} to {to}, as it has no snapshot in common yet",
            snap.name
        )),
    }
    send.push(format!("{from}/{}", snap.name));
    let code = ctx
        .shell
        .pipe(
            &send,
            &cmd(["btrfs", "receive", to]),
            &Env::new(),
            &|stream, text| log.line(stream, text),
        )
        .await;
    match code {
        0 => Ok(()),
        _ => Err(format!(
            "sending {} to {to} failed (exit {code})",
            snap.name
        )),
    }
}

async fn clean(ctx: &Context, snapshot_path: &str, name: &str, log: &Log) -> Result<(), String> {
    let remote = btrfs::inventory(ctx, snapshot_path, name).await?;
    let half: Vec<&Snap> = half_received(&remote);
    if half.is_empty() {
        return Ok(());
    }
    delete(
        ctx,
        snapshot_path,
        &half,
        &format!("Deleting what an interrupted send left in {snapshot_path}"),
        log,
    )
    .await
}

async fn prune(
    ctx: &Context,
    snapshot_path: &str,
    snaps: &[Snap],
    retention: &Retention,
    protected: &BTreeSet<String>,
    log: &Log,
) -> Result<(), String> {
    let doomed = doomed(retention, snaps, protected);
    if doomed.is_empty() {
        return Ok(());
    }
    delete(
        ctx,
        snapshot_path,
        &doomed,
        &format!(
            "Pruning {} of {} snapshots in {snapshot_path}",
            doomed.len(),
            snaps.len()
        ),
        log,
    )
    .await
}

async fn delete(
    ctx: &Context,
    snapshot_path: &str,
    snaps: &[&Snap],
    label: &str,
    log: &Log,
) -> Result<(), String> {
    let mut command = cmd(["btrfs", "subvolume", "delete"]);
    command.extend(
        snaps
            .iter()
            .map(|snap| format!("{snapshot_path}/{}", snap.name)),
    );
    hooks::exec(ctx, &command, label, log)
        .await
        .map_err(|failure| failure.message)
}

/// The newest local snapshot that `remote` holds a complete copy of, received from it
fn parent<'a>(local: &'a [Snap], remote: &[Snap]) -> Option<&'a Snap> {
    local.iter().rev().find(|snap| {
        remote.iter().any(|copy| {
            copy.name == snap.name
                && copy.read_only
                && copy.received.as_deref() == Some(snap.uuid.as_str())
        })
    })
}

/// btrfs marks a copy received, and read-only, only once all of it arrived
fn half_received(remote: &[Snap]) -> Vec<&Snap> {
    remote
        .iter()
        .filter(|snap| snap.received.is_none() && !snap.read_only)
        .collect()
}

fn doomed<'a>(
    retention: &Retention,
    snaps: &'a [Snap],
    protected: &BTreeSet<String>,
) -> Vec<&'a Snap> {
    let newest_first: Vec<&Snap> = snaps.iter().rev().collect();
    let times: Vec<NaiveDateTime> = newest_first.iter().map(|snap| snap.time).collect();
    newest_first
        .into_iter()
        .zip(retention.keep(&times))
        .filter(|(snap, keep)| !keep && !protected.contains(&snap.name))
        .map(|(snap, _)| snap)
        .collect()
}

fn next_name(name: &str, now: NaiveDateTime, taken: &BTreeSet<String>) -> String {
    let stamped = format!("{name}.{}", now.format("%Y%m%dT%H%M"));
    (0..)
        .map(|nr| {
            if nr == 0 {
                stamped.clone()
            } else {
                format!("{stamped}_{nr}")
            }
        })
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused name")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::config::testing;
    use crate::models::atlas::ServiceConfig;
    use crate::shell::FakeShell;

    fn snap(name: &str, uuid: &str, received: Option<&str>) -> Snap {
        Snap {
            name: format!("@wiki.{name}"),
            time: NaiveDateTime::parse_from_str(&format!("{name}00"), "%Y%m%dT%H%M%S").unwrap(),
            uuid: uuid.into(),
            received: received.map(Into::into),
            read_only: true,
        }
    }

    #[test]
    fn should_pick_the_newest_snapshot_whose_copy_was_received_from_it() {
        let local = [
            snap("20261001T0100", "l1", None),
            snap("20261001T0200", "l2", None),
            snap("20261001T0300", "l3", None),
        ];
        let remote = [
            snap("20261001T0100", "r1", Some("l1")),
            snap("20261001T0200", "r2", Some("l2")),
        ];
        assert_eq!(
            parent(&local, &remote).map(|snap| snap.uuid.as_str()),
            Some("l2")
        );
    }

    #[test]
    fn should_not_take_a_copy_by_its_name_alone() {
        let local = [
            snap("20261001T0100", "l1", None),
            snap("20261001T0200", "l2-made-again", None),
        ];
        let mut half = snap("20261001T0100", "r1", Some("l1"));
        half.read_only = false;
        // the 02:00 copy came from an earlier snapshot of the same name, and the 01:00 one is incomplete
        let remote = [half, snap("20261001T0200", "r2", Some("l2"))];
        assert_eq!(parent(&local, &remote), None);
        assert_eq!(parent(&local, &[]), None);
    }

    #[test]
    fn should_only_call_a_copy_half_received_when_btrfs_never_finished_it() {
        let mut half = snap("20261001T0300", "r3", None);
        half.read_only = false;
        let made_by_hand = snap("20261001T0200", "r2", None);
        let remote = [snap("20261001T0100", "r1", Some("l1")), made_by_hand, half];
        assert_eq!(
            half_received(&remote)
                .iter()
                .map(|snap| snap.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["r3"]
        );
    }

    #[test]
    fn should_prune_by_retention_but_spare_a_protected_snapshot() {
        let snaps: Vec<Snap> = (1..=5)
            .map(|hour| snap(&format!("20261001T0{hour}00"), &format!("l{hour}"), None))
            .collect();
        let retention = Retention {
            keep_last: 2,
            ..Default::default()
        };
        let names = |protected: &[&str]| {
            let protected = protected
                .iter()
                .map(|uuid| format!("@wiki.20261001T0{}00", &uuid[1..]))
                .collect();
            doomed(&retention, &snaps, &protected)
                .iter()
                .map(|snap| snap.uuid.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&[]), vec!["l3", "l2", "l1"]);
        assert_eq!(names(&["l2"]), vec!["l3", "l1"]);
    }

    #[test]
    fn should_number_a_second_snapshot_in_the_same_minute() {
        let now = NaiveDateTime::parse_from_str("20261001T030512", "%Y%m%dT%H%M%S").unwrap();
        let taken = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(next_name("@wiki", now, &taken(&[])), "@wiki.20261001T0305");
        assert_eq!(
            next_name(
                "@wiki",
                now,
                &taken(&["@wiki.20261001T0305", "@wiki.20261001T0305_1"])
            ),
            "@wiki.20261001T0305_2"
        );
    }

    struct Disks {
        _dir: tempfile::TempDir,
        root: String,
        ctx: Context,
        entry: Entry,
    }

    impl Disks {
        /// disk-a holds the live @wiki and the snapshots beside it; disk-b and disk-c are other filesystems
        fn new(keep_last: u32) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            for disk in ["disk-a", "disk-b", "disk-c"] {
                std::fs::create_dir_all(format!("{root}/{disk}/.snapshots")).unwrap();
                std::fs::write(format!("{root}/{disk}/.fake-btrfs-filesystem"), disk).unwrap();
            }
            std::fs::create_dir_all(format!("{root}/disk-a/@wiki")).unwrap();
            std::fs::write(format!("{root}/disk-a/@wiki/page"), "first").unwrap();
            let config = BtrfsConfig {
                subvolume: format!("{root}/disk-a/@wiki"),
                snapshot_paths: ["disk-b", "disk-a", "disk-c"]
                    .iter()
                    .map(|disk| format!("{root}/{disk}/.snapshots"))
                    .collect(),
                retention: Retention {
                    keep_last,
                    ..Default::default()
                },
                schedule: None,
                lifecycle: None,
            };
            let entry = Entry {
                path: format!("{root}/bacre.yaml"),
                config: ServiceConfig {
                    service: "wiki".into(),
                    home: root.clone(),
                    btrfs: Some(config),
                    restic: None,
                },
            };
            let ctx = Context {
                config: Arc::new(testing::config("")),
                shell: Arc::new(FakeShell::default()),
            };
            Self {
                _dir: dir,
                root,
                ctx,
                entry,
            }
        }

        async fn run(&self, at: &str) -> (Outcome, Vec<String>) {
            let lines = Arc::new(Mutex::new(Vec::new()));
            let sink = lines.clone();
            let log = Log::new(move |_, text| sink.lock().unwrap().push(text));
            let now = NaiveDateTime::parse_from_str(&format!("{at}00"), "%Y%m%dT%H%M%S").unwrap();
            let config = self.entry.config.btrfs.as_ref().unwrap();
            let outcome = snapshot(&self.ctx, &self.entry, config, None, now, &log).await;
            let lines = lines.lock().unwrap().clone();
            (outcome, lines)
        }

        fn on(&self, disk: &str) -> Vec<String> {
            let mut names: Vec<String> =
                std::fs::read_dir(format!("{}/{disk}/.snapshots", self.root))
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .filter(|name| name.starts_with('@'))
                    .map(|name| name.trim_start_matches("@wiki.20261001T").to_string())
                    .collect();
            names.sort();
            names
        }

        fn path(&self, relative: &str) -> std::path::PathBuf {
            Path::new(&self.root).join(relative)
        }
    }

    #[tokio::test]
    async fn should_snapshot_locally_send_everywhere_else_and_only_the_changes_after_the_first() {
        let disks = Disks::new(5);

        let (first, lines) = disks.run("20261001T0100").await;
        assert!(first.is_ok(), "{first:?} {lines:#?}");
        assert!(
            lines
                .iter()
                .filter(|line| line.contains("as it has no snapshot in common yet"))
                .count()
                == 2,
            "{lines:#?}"
        );

        std::fs::write(disks.path("disk-a/@wiki/page"), "second").unwrap();
        let (second, lines) = disks.run("20261001T0200").await;
        assert!(second.is_ok(), "{second:?} {lines:#?}");
        assert!(
            lines
                .iter()
                .filter(|line| line.contains("as the changes since @wiki.20261001T0100"))
                .count()
                == 2,
            "{lines:#?}"
        );

        for disk in ["disk-a", "disk-b", "disk-c"] {
            assert_eq!(disks.on(disk), vec!["0100", "0200"], "{disk}");
        }
        assert_eq!(
            std::fs::read_to_string(disks.path("disk-c/.snapshots/@wiki.20261001T0200/page"))
                .unwrap(),
            "second"
        );
    }

    #[tokio::test]
    async fn should_go_on_without_a_snapshot_path_that_failed_and_clear_its_half_copy() {
        let disks = Disks::new(5);
        disks.run("20261001T0100").await.0.unwrap();

        std::fs::write(disks.path("disk-b/.snapshots/.fake-btrfs-fail-receive"), "").unwrap();
        let (failed, lines) = disks.run("20261001T0200").await;
        assert_eq!(
            failed.unwrap_err().message,
            "sending @wiki.20261001T0200 to ".to_string()
                + &disks.root
                + "/disk-b/.snapshots failed (exit 1)",
            "{lines:#?}"
        );
        assert_eq!(disks.on("disk-a"), vec!["0100", "0200"]);
        assert_eq!(disks.on("disk-b"), vec!["0100"]);
        assert_eq!(disks.on("disk-c"), vec!["0100", "0200"]);

        std::fs::remove_file(disks.path("disk-b/.snapshots/.fake-btrfs-fail-receive")).unwrap();
        let (recovered, lines) = disks.run("20261001T0300").await;
        assert!(recovered.is_ok(), "{recovered:?} {lines:#?}");
        assert!(lines.iter().any(|line| line.contains(&format!("Sending @wiki.20261001T0300 to {}/disk-b/.snapshots, as the changes since @wiki.20261001T0100", disks.root))), "{lines:#?}");
    }

    #[tokio::test]
    async fn should_prune_every_snapshot_path_but_keep_the_parent_a_lagging_one_needs() {
        let disks = Disks::new(2);
        disks.run("20261001T0100").await.0.unwrap();

        // disk-b is out of reach for two runs, so its newest copy in common stays at 01:00
        std::fs::write(disks.path("disk-b/.snapshots/.fake-btrfs-fail-receive"), "").unwrap();
        let _ = disks.run("20261001T0200").await;
        let _ = disks.run("20261001T0300").await;
        assert_eq!(disks.on("disk-a"), vec!["0100", "0200", "0300"]);
        assert_eq!(disks.on("disk-c"), vec!["0200", "0300"]);

        std::fs::remove_file(disks.path("disk-b/.snapshots/.fake-btrfs-fail-receive")).unwrap();
        let (caught_up, lines) = disks.run("20261001T0400").await;
        assert!(caught_up.is_ok(), "{caught_up:?} {lines:#?}");
        assert!(
            lines.iter().any(|line| line
                .ends_with("disk-b/.snapshots, as the changes since @wiki.20261001T0100")),
            "{lines:#?}"
        );
        assert_eq!(disks.on("disk-a"), vec!["0300", "0400"]);
        assert_eq!(disks.on("disk-b"), vec!["0100", "0400"]);
        assert_eq!(disks.on("disk-c"), vec!["0300", "0400"]);
    }

    #[tokio::test]
    async fn should_refuse_when_no_snapshot_path_is_on_the_subvolumes_filesystem() {
        let mut disks = Disks::new(2);
        let config = disks.entry.config.btrfs.as_mut().unwrap();
        config
            .snapshot_paths
            .retain(|snapshot_path| !snapshot_path.contains("disk-a"));
        let (refused, _) = disks.run("20261001T0100").await;
        assert!(
            refused
                .unwrap_err()
                .message
                .starts_with("none of the snapshot paths is on the filesystem of")
        );
        assert!(disks.on("disk-b").is_empty());
    }
}
