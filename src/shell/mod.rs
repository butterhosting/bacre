//! The one I/O boundary of the daemon: every backend talks to its tool through this.
//! The real one spawns processes; the fake one answers from generated data; the sandbox one
//! (dev stage) is real except for btrfs.

mod fake;
mod real;
mod sandbox;

use std::collections::BTreeMap;

use async_trait::async_trait;

pub use fake::FakeShell;
pub use real::RealShell;
pub use sandbox::SandboxShell;

pub type Env = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

pub type OnLine<'a> = &'a (dyn Fn(Stream, &str) + Send + Sync);

#[async_trait]
pub trait Shell: Send + Sync {
    /// Run to completion and hand back everything it printed
    async fn run(&self, cmd: &[String], env: &Env) -> Output;

    /// Run while handing over every line as it is printed; gives the exit code
    async fn stream(
        &self,
        cmd: &[String],
        cwd: Option<&str>,
        env: &Env,
        on_line: OnLine<'_>,
    ) -> i32;
}

/// A command line from its parts
pub fn cmd<const N: usize>(parts: [&str; N]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}
