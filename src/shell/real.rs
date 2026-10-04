use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

use super::{Env, OnLine, Output, Shell, Stream};

/// `env` is added to the daemon's own environment, never instead of it.
pub struct RealShell;

const NOT_STARTED: i32 = 127;

#[async_trait]
impl Shell for RealShell {
    async fn run(&self, cmd: &[String], env: &Env) -> Output {
        let Some((program, args)) = cmd.split_first() else {
            return not_started("", "empty command");
        };
        match Command::new(program)
            .args(args)
            .envs(env)
            .stdin(Stdio::null())
            .output()
            .await
        {
            Ok(output) => Output {
                code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Err(e) => not_started(program, &e.to_string()),
        }
    }

    async fn stream(
        &self,
        cmd: &[String],
        cwd: Option<&str>,
        env: &Env,
        on_line: OnLine<'_>,
    ) -> i32 {
        let Some((program, args)) = cmd.split_first() else {
            on_line(Stream::Err, "empty command");
            return NOT_STARTED;
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                on_line(Stream::Err, &format!("{program}: {e}"));
                return NOT_STARTED;
            }
        };
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        tokio::join!(
            lines(stdout, Stream::Out, on_line),
            lines(stderr, Stream::Err, on_line)
        );
        match child.wait().await {
            Ok(status) => status.code().unwrap_or(-1),
            Err(e) => {
                on_line(Stream::Err, &format!("{program}: {e}"));
                -1
            }
        }
    }
}

fn not_started(program: &str, reason: &str) -> Output {
    Output {
        code: NOT_STARTED,
        stdout: String::new(),
        stderr: format!("{program}: {reason}"),
    }
}

async fn lines(reader: impl AsyncRead + Unpin, stream: Stream, on_line: OnLine<'_>) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&buffer);
                on_line(stream, text.trim_end_matches('\n').trim_end_matches('\r'));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::shell::cmd;

    #[tokio::test]
    async fn should_run_a_command_and_hand_back_what_it_printed() {
        let output = RealShell
            .run(
                &cmd(["sh", "-c", "echo out; echo err >&2; exit 3"]),
                &Env::new(),
            )
            .await;
        assert_eq!(
            (output.code, output.stdout.as_str(), output.stderr.as_str()),
            (3, "out\n", "err\n")
        );
    }

    #[tokio::test]
    async fn should_add_to_the_environment_rather_than_replace_it() {
        let env = Env::from([("BACRE_TEST".to_string(), "yes".to_string())]);
        let output = RealShell
            .run(
                &cmd(["sh", "-c", "echo $BACRE_TEST; test -n \"$PATH\""]),
                &env,
            )
            .await;
        assert_eq!((output.code, output.stdout.as_str()), (0, "yes\n"));
    }

    #[tokio::test]
    async fn should_stream_lines_from_both_streams_in_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        let seen = Mutex::new(Vec::new());
        let code = RealShell
            .stream(
                &cmd([
                    "sh",
                    "-c",
                    "basename \"$PWD\"; echo warn >&2; printf 'no newline'",
                ]),
                Some(dir.path().to_str().unwrap()),
                &Env::new(),
                &|stream, text| seen.lock().unwrap().push((stream, text.to_string())),
            )
            .await;
        let mut seen = seen.into_inner().unwrap();
        seen.sort_by_key(|(stream, _)| *stream == Stream::Err);

        assert_eq!(code, 0);
        assert_eq!(
            seen,
            vec![
                (
                    Stream::Out,
                    dir.path()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                ),
                (Stream::Out, "no newline".to_string()),
                (Stream::Err, "warn".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn should_report_a_command_that_does_not_exist_as_a_failed_one() {
        let output = RealShell
            .run(&cmd(["bacre-no-such-command"]), &Env::new())
            .await;
        assert_eq!(output.code, 127);
        assert!(output.stderr.starts_with("bacre-no-such-command: "));
    }
}
