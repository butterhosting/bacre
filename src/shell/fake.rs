use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Local, SecondsFormat, TimeZone, Utc};
use serde_json::json;

use super::{Env, OnLine, Output, RealShell, Shell, Stream};

/// Answers the exact invocations the backends make, in their tools' output formats.
#[derive(Default)]
pub struct FakeShell {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    created_restic: Vec<(String, DateTime<Local>)>,
    initialized_repos: HashSet<String>,
}

const LOCAL_ONLY: [&str; 3] = ["dns", "radio", "recipes"];
const BROKEN: [&str; 1] = ["gallery"];
const STALE: [&str; 1] = ["ledger"];
const LIVE: &str = "/srv/demo/disk-a";
const DUMPS: &str = "/srv/demo/dumps";

#[async_trait]
impl Shell for FakeShell {
    async fn run(&self, cmd: &[String], env: &Env) -> Output {
        println!("==> (fake) {}", cmd.join(" "));
        match cmd.first().map(String::as_str) {
            Some("btrfs" | "findmnt") => RealShell.run(&script(cmd), env).await,
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
        env: &Env,
        on_line: OnLine<'_>,
    ) -> i32 {
        let line: String = cmd.join(" ").chars().take(120).collect();
        println!(
            "==> (fake, streaming{}) {line}",
            cwd.map(|cwd| format!(" in {cwd}")).unwrap_or_default()
        );
        match cmd.first().map(String::as_str) {
            Some("bash") => self.bash(cmd, on_line).await,
            Some("restic") => self.restic_streaming(cmd, on_line).await,
            Some("btrfs") => RealShell.stream(&script(cmd), cwd, env, on_line).await,
            // a subvolume is a directory here, so it really moves
            Some("mv") => match cmd {
                [_, from, to] => match tokio::fs::rename(from, to).await {
                    Ok(()) => 0,
                    Err(e) => {
                        on_line(Stream::Err, &format!("mv: {e}"));
                        1
                    }
                },
                _ => 1,
            },
            other => {
                on_line(
                    Stream::Err,
                    &format!("{}: command not found", other.unwrap_or_default()),
                );
                127
            }
        }
    }

    async fn pipe(&self, from: &[String], to: &[String], env: &Env, on_line: OnLine<'_>) -> i32 {
        RealShell
            .pipe(&script(from), &script(to), env, on_line)
            .await
    }
}

impl FakeShell {
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

/// btrfs and findmnt are the fakes in fake/, which work on plain directories
fn script(cmd: &[String]) -> Vec<String> {
    let mut cmd = cmd.to_vec();
    if let Some(program @ ("btrfs" | "findmnt")) = cmd.first().map(String::as_str) {
        cmd[0] = format!("{}/fake/{program}.sh", env!("CARGO_MANIFEST_DIR"));
    }
    cmd
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

fn after<'a>(cmd: &'a [String], flag: &str) -> &'a str {
    cmd.iter()
        .position(|part| part == flag)
        .and_then(|index| cmd.get(index + 1))
        .map(String::as_str)
        .unwrap_or_default()
}

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

async fn pause(base: u64, jitter: u64) {
    let extra = if jitter == 0 {
        0
    } else {
        u64::from(Local::now().timestamp_subsec_nanos()) % jitter
    };
    tokio::time::sleep(Duration::from_millis(base + extra)).await;
}

fn days_ago(days: i64, hour: u32, minute: u32) -> DateTime<Local> {
    wall(Local::now() - chrono::Duration::days(days), hour, minute, 4)
}

fn wall(day: DateTime<Local>, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
    let naive = day
        .date_naive()
        .and_hms_opt(hour, minute, second)
        .expect("a valid time");
    Local.from_local_datetime(&naive).earliest().unwrap_or(day)
}

fn stamp(time: DateTime<Local>) -> String {
    time.format("%Y%m%dT%H%M").to_string()
}

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
