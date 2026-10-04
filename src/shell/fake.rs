use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Local, SecondsFormat, TimeZone, Timelike, Utc};
use serde_json::json;

use super::{Env, OnLine, Output, Shell, Stream};

/// Stands in for btrfs, btrbk, restic and bash on a machine that has none of them: all of
/// them for the tests, btrfs and btrbk for the dev stage (see `SandboxShell`). It recognises
/// the exact invocations the backends make and answers in their output formats, with enough
/// variety to exercise every state the website can show. Backups it "runs" show up in later
/// listings, so the dev loop feels like the real thing.
#[derive(Default)]
pub struct FakeShell {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    created_btrfs: Vec<(String, DateTime<Local>)>,
    created_restic: Vec<(String, DateTime<Local>)>,
    initialized_repos: HashSet<String>,
}

// A made-up machine, matching the services the sandbox is seeded with: nothing here is named after a real deployment
const SERVICES: [&str; 8] = [
    "dns", "gallery", "ledger", "mailbox", "radio", "recipes", "tracker", "wiki",
];
/// No restic repository at all
const LOCAL_ONLY: [&str; 3] = ["dns", "radio", "recipes"];
/// The repository listing fails
const BROKEN: [&str; 1] = ["gallery"];
/// Offsite backups stopped nine days ago
const STALE: [&str; 1] = ["ledger"];
const LIVE: &str = "/srv/demo/disk-a";
const DUMPS: &str = "/srv/demo/dumps";

#[async_trait]
impl Shell for FakeShell {
    async fn run(&self, cmd: &[String], _env: &Env) -> Output {
        println!("==> (fake) {}", cmd.join(" "));
        match cmd.first().map(String::as_str) {
            Some("btrfs") => self.btrfs(cmd),
            Some("restic") => self.restic(cmd).await,
            other => output(
                127,
                "",
                &format!("{}: command not found", other.unwrap_or_default()),
            ),
        }
    }

    async fn stream(
        &self,
        cmd: &[String],
        cwd: Option<&str>,
        _env: &Env,
        on_line: OnLine<'_>,
    ) -> i32 {
        let line: String = cmd.join(" ").chars().take(120).collect();
        println!(
            "==> (fake, streaming{}) {line}",
            cwd.map(|cwd| format!(" in {cwd}")).unwrap_or_default()
        );
        match cmd.first().map(String::as_str) {
            Some("bash") => self.bash(cmd, on_line).await,
            Some("btrbk") => self.btrbk(cmd, on_line).await,
            Some("restic") => self.restic_streaming(cmd, on_line).await,
            Some("btrfs") => self.btrfs_streaming(cmd, on_line).await,
            Some("mv") => {
                pause(150, 0).await;
                0
            }
            other => {
                on_line(
                    Stream::Err,
                    &format!("{}: command not found", other.unwrap_or_default()),
                );
                127
            }
        }
    }
}

impl FakeShell {
    /// `btrfs subvolume list -s <dir>`: one line per snapshot subvolume
    fn btrfs(&self, cmd: &[String]) -> Output {
        let dir = cmd.last().map(String::as_str).unwrap_or_default();
        let verb = (
            cmd.get(1).map(String::as_str),
            cmd.get(2).map(String::as_str),
        );
        if verb == (Some("subvolume"), Some("show")) {
            // every snapshot the listing shows exists, as far as the dev stage is concerned
            return if dir.contains("/@") {
                output(0, dir, "")
            } else {
                output(1, "", &format!("ERROR: Could not find subvolume {dir}"))
            };
        }
        if verb != (Some("subvolume"), Some("list")) {
            return output(1, "", "btrfs: unsupported fake invocation");
        }
        // the second disk, in the tests' made-up paths and in the seeded sandbox alike
        let is_replica = dir.contains("/disk-b/");
        let mut lines = Vec::new();
        let mut line = |service: &str, time: DateTime<Local>| {
            let id = 300 + lines.len();
            lines.push(format!(
                "ID {id} gen {} cgen {} top level 5 otime {} path .snapshots/@{service}.{}",
                id * 10,
                id * 10 - 7,
                time.format("%Y-%m-%d %H:%M:%S"),
                stamp(time)
            ));
        };
        for service in SERVICES {
            // hourly for the last 36 hours; the replica lags one snapshot behind
            for hours_ago in (if is_replica { 1 } else { 0 })..36 {
                line(service, self::hours_ago(hours_ago, 5));
            }
        }
        for (service, time) in &self.state.lock().unwrap().created_btrfs {
            line(service, *time);
        }
        output(0, &(lines.join("\n") + "\n"), "")
    }

    /// `restic -r <repo> … snapshots --json` / `cat config` / `init`
    async fn restic(&self, cmd: &[String]) -> Output {
        let repo = after(cmd, "-r");
        let service = repo.rsplit('/').next().unwrap_or_default().to_string();
        pause(300, 400).await; // S3 round trips are not free
        let has = |word: &str| cmd.iter().any(|part| part == word);

        let absent = LOCAL_ONLY.contains(&service.as_str())
            && !self
                .state
                .lock()
                .unwrap()
                .initialized_repos
                .contains(&service);
        if has("init") {
            self.state.lock().unwrap().initialized_repos.insert(service);
            return output(0, &format!("created restic repository at {repo}\n"), "");
        }
        if absent {
            return output(
                10,
                "",
                &format!(
                    "Fatal: repository does not exist: unable to open config file: Stat: {repo}/config: no such file"
                ),
            );
        }
        if BROKEN.contains(&service.as_str()) {
            return output(1, "", &broken(repo));
        }
        if has("cat") {
            return output(0, "{}", "");
        }

        let snapshot = |time: DateTime<Local>, seed: String| {
            json!({
                "time": time.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true),
                "short_id": hex(&seed),
                "tags": [format!("t={}", stamp(time))],
                "paths": [format!("{LIVE}/@{service}/data"), format!("{DUMPS}/{service}")],
                "hostname": "demobox",
            })
        };
        let mut snapshots = Vec::new();
        for (created, time) in &self.state.lock().unwrap().created_restic {
            if *created == service {
                snapshots.push(snapshot(
                    *time,
                    format!("{service}-{}", time.timestamp_millis()),
                ));
            }
        }
        let start = if STALE.contains(&service.as_str()) {
            9
        } else {
            0
        };
        for days_ago in start..start + 30 {
            snapshots.push(snapshot(
                self::days_ago(days_ago, 3, 1),
                format!("{service}-{days_ago}"),
            ));
        }
        output(0, &serde_json::Value::Array(snapshots).to_string(), "")
    }

    /// `bash -euo pipefail -c <script>`: echoes each line as bash -x would, and "runs" it
    async fn bash(&self, cmd: &[String], on_line: OnLine<'_>) -> i32 {
        // a trailing backslash continues the command on the next line, as it does for the real bash
        let script = join_continuations(after(cmd, "-c"));
        for line in script
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            on_line(Stream::Err, &format!("+ {line}"));
            pause(150, 250).await;
            if line.starts_with("docker compose up") {
                on_line(Stream::Out, " ✔ Container main  Healthy");
            } else if line.starts_with("docker compose stop")
                || line.starts_with("docker compose down")
            {
                on_line(Stream::Out, " ✔ Container main  Stopped");
            } else if line.contains("false") {
                on_line(
                    Stream::Err,
                    "bash: the fake shell fails on purpose when a hook says `false`",
                );
                return 1;
            }
        }
        0
    }

    /// `btrbk -c <config> run <subvolume…>`: narrates a snapshot + send/receive per subvolume
    async fn btrbk(&self, cmd: &[String], on_line: OnLine<'_>) -> i32 {
        let subvolumes = cmd.iter().skip_while(|part| *part != "run").skip(1);
        let now = Local::now();
        on_line(Stream::Out, &"-".repeat(80));
        on_line(
            Stream::Out,
            "Backup Summary (btrbk command line client, version 0.32.6)",
        );
        // where the snapshot goes and where it is sent, as the generated configuration says
        let config = tokio::fs::read_to_string(after(cmd, "-c"))
            .await
            .unwrap_or_default();
        let settings = |key: &str| -> Vec<String> {
            config
                .lines()
                .filter_map(|line| line.trim().strip_prefix(key))
                .map(|value| value.trim().to_string())
                .collect()
        };
        let volume = settings("volume ")
            .pop()
            .unwrap_or_else(|| LIVE.to_string());
        let snapshot_dir = settings("snapshot_dir ")
            .pop()
            .unwrap_or_else(|| ".snapshots".to_string());
        let targets = settings("target ");
        for subvolume in subvolumes {
            let name = format!("{subvolume}.{}", stamp(now));
            pause(400, 400).await;
            on_line(Stream::Out, &format!("+++ {volume}/{snapshot_dir}/{name}"));
            for target in &targets {
                pause(600, 800).await;
                on_line(Stream::Out, &format!(">>> {target}/{name}"));
            }
            let service = subvolume.trim_start_matches('@').to_string();
            self.state
                .lock()
                .unwrap()
                .created_btrfs
                .push((service, now));
        }
        on_line(Stream::Out, "");
        on_line(Stream::Out, "NOTE: Dryrun (fake): no retention applied");
        0
    }

    /// `btrfs subvolume snapshot <src> <dst>` and `btrfs subvolume delete <path>`, as the restore runs them
    async fn btrfs_streaming(&self, cmd: &[String], on_line: OnLine<'_>) -> i32 {
        pause(300, 500).await;
        let arg = |index: usize| cmd.get(index).map(String::as_str).unwrap_or_default();
        match arg(2) {
            "snapshot" => {
                on_line(
                    Stream::Out,
                    &format!("Create snapshot of '{}' in '{}'", arg(3), arg(4)),
                );
                0
            }
            "delete" => {
                on_line(
                    Stream::Out,
                    &format!("Delete subvolume 256 (no-commit): '{}'", arg(3)),
                );
                0
            }
            _ => {
                on_line(Stream::Err, "btrfs: unsupported fake invocation");
                1
            }
        }
    }

    /// `restic … restore <id> --target <dir>`: really creates the directory, with a small tree under the absolute paths
    async fn restic_restore(&self, cmd: &[String], service: &str, on_line: OnLine<'_>) -> i32 {
        let target = Path::new(after(cmd, "--target"));
        let id = after(cmd, "restore");
        on_line(
            Stream::Out,
            "repository opened (version 2, compression level auto)",
        );
        on_line(
            Stream::Out,
            &format!(
                "restoring snapshot {id} of [{LIVE}/@{service}/data {DUMPS}/{service}] to {}",
                target.display()
            ),
        );
        let data = target.join(format!("{}/@{service}/data", &LIVE[1..]));
        let dumps = target.join(format!("{}/{service}", &DUMPS[1..]));
        let written = async {
            tokio::fs::create_dir_all(&data).await?;
            tokio::fs::create_dir_all(&dumps).await?;
            tokio::fs::write(
                data.join("README.txt"),
                format!("Fake download of {service}/{id}, made by the dev stage.\n"),
            )
            .await?;
            tokio::fs::write(dumps.join(format!("{service}.sql")), "-- fake dump\n").await
        };
        if let Err(e) = written.await {
            on_line(Stream::Err, &format!("Fatal: {e}"));
            return 1;
        }
        for quarter in 1..=4 {
            pause(500, 0).await;
            let pct = quarter * 25;
            on_line(
                Stream::Out,
                &format!(
                    "[0:0{quarter}] {pct}%  {} files {:.2} GiB",
                    pct * 180,
                    f64::from(pct) * 0.014
                ),
            );
        }
        on_line(
            Stream::Out,
            "Summary: Restored 18204 files/dirs (1.410 GiB) in 0:04",
        );
        0
    }

    /// `restic -r <repo> … backup <paths…>` and `forget --prune`
    async fn restic_streaming(&self, cmd: &[String], on_line: OnLine<'_>) -> i32 {
        let repo = after(cmd, "-r");
        let service = repo.rsplit('/').next().unwrap_or_default();
        let has = |word: &str| cmd.iter().any(|part| part == word);
        if BROKEN.contains(&service) {
            pause(500, 0).await;
            on_line(Stream::Err, &broken(repo));
            return 1;
        }
        if has("restore") {
            return self.restic_restore(cmd, service, on_line).await;
        }
        if has("backup") {
            on_line(Stream::Out, "open repository");
            on_line(Stream::Out, "lock repository");
            on_line(Stream::Out, "load index files");
            for quarter in 0..=4u32 {
                pause(400, 0).await;
                let pct = quarter * 25;
                on_line(
                    Stream::Out,
                    &format!(
                        "[0:0{quarter}] {pct}%  {} files 1.{pct} GiB",
                        (f64::from(pct) * 1.4).round()
                    ),
                );
            }
            let time = Local::now();
            on_line(
                Stream::Out,
                &format!(
                    "snapshot {} saved",
                    hex(&format!("{service}-{}", time.timestamp_millis()))
                ),
            );
            self.state
                .lock()
                .unwrap()
                .created_restic
                .push((service.to_string(), time));
            return 0;
        }
        if has("forget") {
            pause(600, 0).await;
            on_line(
                Stream::Out,
                "Applying Policy: keep 3 latest, 30 daily, 15 weekly, 12 monthly snapshots",
            );
            on_line(Stream::Out, "keep 31 snapshots, remove 1 snapshots");
            on_line(Stream::Out, "1 snapshots have been removed, running prune");
            pause(600, 0).await;
            on_line(Stream::Out, "done");
            return 0;
        }
        on_line(Stream::Err, "restic: unsupported fake invocation");
        1
    }
}

fn output(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn broken(repo: &str) -> String {
    format!(
        "Fatal: unable to open repository at {repo}: client.BucketExists: operation error S3: HeadBucket, request timed out"
    )
}

/// The argument that follows `flag`, or nothing
fn after<'a>(cmd: &'a [String], flag: &str) -> &'a str {
    cmd.iter()
        .position(|part| part == flag)
        .and_then(|index| cmd.get(index + 1))
        .map(String::as_str)
        .unwrap_or_default()
}

/// A backslash at the end of a line, and the indentation of the next, removed
fn join_continuations(script: &str) -> String {
    let mut joined = String::new();
    let mut rest = script;
    while let Some(index) = rest.find("\\\n") {
        joined.push_str(&rest[..index]);
        rest = rest[index + 2..].trim_start();
    }
    joined.push_str(rest);
    joined
}

/// Waits `base` milliseconds and up to `jitter` more, so the output arrives unevenly
async fn pause(base: u64, jitter: u64) {
    let extra = if jitter == 0 {
        0
    } else {
        u64::from(Local::now().timestamp_subsec_nanos()) % jitter
    };
    tokio::time::sleep(Duration::from_millis(base + extra)).await;
}

fn hours_ago(hours: i64, minute: u32) -> DateTime<Local> {
    let then = Local::now() - chrono::Duration::hours(hours);
    wall(then, then.hour(), minute, 12)
}

fn days_ago(days: i64, hour: u32, minute: u32) -> DateTime<Local> {
    wall(Local::now() - chrono::Duration::days(days), hour, minute, 4)
}

/// The same day as `day`, at the given time on the clock
fn wall(day: DateTime<Local>, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
    let naive = day
        .date_naive()
        .and_hms_opt(hour, minute, second)
        .expect("a valid time");
    Local.from_local_datetime(&naive).earliest().unwrap_or(day)
}

/// btrbk's snapshot stamp, local time: 20261001T0305
fn stamp(time: DateTime<Local>) -> String {
    time.format("%Y%m%dT%H%M").to_string()
}

/// A stable 8-hex id from a seed, so refreshes return the same snapshots
fn hex(seed: &str) -> String {
    // FNV-1a: small, and the same on every run and machine
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_join_a_command_that_continues_on_the_next_line() {
        assert_eq!(
            join_continuations("rsync -a \\\n    /from \\\n    /to\nnext"),
            "rsync -a /from /to\nnext"
        );
    }

    #[test]
    fn should_give_the_same_id_for_the_same_seed() {
        assert_eq!(hex("wiki-3"), hex("wiki-3"));
        assert_ne!(hex("wiki-3"), hex("wiki-4"));
        assert_eq!(hex("wiki-3").len(), 8);
    }
}
