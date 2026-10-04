use chrono::{Duration, Utc};

use super::job_service::JobService;
use crate::models::archives::iso;
use crate::models::jobs::{
    BackupRequest, BtrfsTarget, Job, Line, LineStream, Mode, Request, Status, Trigger,
};

pub fn seed(job_service: &JobService) {
    use LineStream::{Err, Info, Out};

    job_service.seed(job(Sample {
        id: "dev-0001",
        request: Request::Backup(BackupRequest::Btrfs {
            targets: vec![btrfs("wiki", Mode::Hot), btrfs("mailbox", Mode::Hot)],
        }),
        minutes_ago: 190,
        seconds: 7,
        error: None,
        lines: &[
            (Info, "==> Snapshotting wiki (@wiki) with btrbk"),
            (Out, "+++ /srv/demo/disk-a/.snapshots/@wiki.20261002T1150"),
            (Out, ">>> /srv/demo/disk-b/.snapshots/@wiki.20261002T1150"),
            (Info, "==> Snapshotting mailbox (@mailbox) with btrbk"),
            (
                Out,
                "+++ /srv/demo/disk-a/.snapshots/@mailbox.20261002T1150",
            ),
            (
                Out,
                ">>> /srv/demo/disk-b/.snapshots/@mailbox.20261002T1150",
            ),
            (Info, "==> Refreshing the listings"),
            (Info, "==> Done"),
        ],
    }));

    job_service.seed(job(Sample {
        id: "dev-0002",
        request: Request::Backup(BackupRequest::Btrfs {
            targets: vec![btrfs("gallery", Mode::Cold)],
        }),
        minutes_ago: 95,
        seconds: 41,
        error: Some("Starting gallery failed (exit 1)"),
        lines: &[
            (Info, "==> Stopping gallery"),
            (Err, "+ docker compose down"),
            (Out, " ✔ Container main  Stopped"),
            (Info, "==> Snapshotting gallery (@gallery) with btrbk"),
            (
                Out,
                "+++ /srv/demo/disk-a/.snapshots/@gallery.20261002T1325",
            ),
            (
                Out,
                ">>> /srv/demo/disk-b/.snapshots/@gallery.20261002T1325",
            ),
            (Info, "==> Starting gallery"),
            (Err, "+ docker compose up --wait"),
            (Err, "container gallery-worker-1 is unhealthy"),
            (Info, "==> Failed: Starting gallery failed (exit 1)"),
        ],
    }));
}

struct Sample {
    id: &'static str,
    request: Request,
    minutes_ago: i64,
    seconds: i64,
    error: Option<&'static str>,
    lines: &'static [(LineStream, &'static str)],
}

fn btrfs(service: &str, mode: Mode) -> BtrfsTarget {
    BtrfsTarget {
        service: service.to_string(),
        mode,
    }
}

fn job(sample: Sample) -> Job {
    let started = Utc::now() - Duration::minutes(sample.minutes_ago);
    let step = Duration::milliseconds(sample.seconds * 1000 / sample.lines.len() as i64);
    Job {
        id: sample.id.to_string(),
        trigger: Trigger::Manual,
        title: sample.request.title(),
        request: sample.request,
        status: if sample.error.is_some() {
            Status::Failed
        } else {
            Status::Succeeded
        },
        started_at: iso(started),
        ended_at: Some(iso(started + Duration::seconds(sample.seconds))),
        error: sample.error.map(str::to_string),
        failed: Vec::new(),
        lines: sample
            .lines
            .iter()
            .enumerate()
            .map(|(index, (stream, text))| Line {
                at: iso(started + step * index as i32),
                stream: *stream,
                text: text.to_string(),
            })
            .collect(),
    }
}
