use async_trait::async_trait;

use super::{Env, FakeShell, OnLine, Output, RealShell, Shell};

/// The shell of the dev stage: everything runs for real (restic, the hooks), except what a
/// container cannot do. A container has no btrfs, so `btrfs`, `btrbk`, and the `mv` that
/// moves a subvolume aside during a btrfs restore are answered by the fake.
#[derive(Default)]
pub struct SandboxShell {
    fake: FakeShell,
}

impl SandboxShell {
    fn is_btrfs(cmd: &[String]) -> bool {
        // `mv` is only ever a btrfs restore's; nothing else Bacre runs moves files with it
        matches!(
            cmd.first().map(String::as_str),
            Some("btrfs" | "btrbk" | "mv")
        )
    }
}

#[async_trait]
impl Shell for SandboxShell {
    async fn run(&self, cmd: &[String], env: &Env) -> Output {
        if Self::is_btrfs(cmd) {
            self.fake.run(cmd, env).await
        } else {
            RealShell.run(cmd, env).await
        }
    }

    async fn stream(
        &self,
        cmd: &[String],
        cwd: Option<&str>,
        env: &Env,
        on_line: OnLine<'_>,
    ) -> i32 {
        if Self::is_btrfs(cmd) {
            self.fake.stream(cmd, cwd, env, on_line).await
        } else {
            RealShell.stream(cmd, cwd, env, on_line).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::cmd;

    #[tokio::test]
    async fn should_run_ordinary_commands_for_real_and_fake_btrfs() {
        let shell = SandboxShell::default();

        let real = shell
            .run(&cmd(["sh", "-c", "echo for real"]), &Env::new())
            .await;
        assert_eq!((real.code, real.stdout.as_str()), (0, "for real\n"));

        let fake = shell
            .run(
                &cmd([
                    "btrfs",
                    "subvolume",
                    "list",
                    "-s",
                    "/srv/sandbox/disk-a/.snapshots",
                ]),
                &Env::new(),
            )
            .await;
        assert_eq!(fake.code, 0);
        assert!(fake.stdout.contains("path .snapshots/@wiki."));

        // the fake mv moves nothing: there is no subvolume to move
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("@wiki");
        std::fs::create_dir(&live).unwrap();
        let aside = dir.path().join("@wiki.bacre-replaced");
        let code = shell
            .stream(
                &cmd(["mv", live.to_str().unwrap(), aside.to_str().unwrap()]),
                None,
                &Env::new(),
                &|_, _| {},
            )
            .await;
        assert_eq!(code, 0);
        assert!(live.is_dir() && !aside.exists());
    }
}
