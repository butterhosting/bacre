//! Runs a bacre.yaml hook: a bash script, in the service's home, with its output in the job log.

use super::Context;
use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;
use crate::models::atlas::Entry;
use crate::shell::{Env, cmd};

pub async fn run(
    ctx: &Context,
    entry: &Entry,
    label: &str,
    script: &str,
    extra_env: &[(&str, &str)],
    log: &Log,
) -> Outcome {
    log.info(format!("==> {label}"));
    let mut env = Env::from([("BACRE_SERVICE".to_string(), entry.config.service.clone())]);
    env.extend(
        extra_env
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    );

    let command = cmd(["bash", "-euo", "pipefail", "-c", script]);
    let code = ctx
        .shell
        .stream(&command, Some(&entry.config.home), &env, &|stream, text| {
            log.line(stream, text)
        })
        .await;
    if code != 0 {
        return Err(Failure::new(format!("{label} failed (exit {code})")));
    }
    Ok(())
}

/// Runs a command that is Bacre's own step rather than a hook, narrated the same way
pub async fn exec(ctx: &Context, command: &[String], label: &str, log: &Log) -> Outcome {
    log.info(format!("==> {label}"));
    let code = ctx
        .shell
        .stream(command, None, &Env::new(), &|stream, text| {
            log.line(stream, text)
        })
        .await;
    if code != 0 {
        return Err(Failure::new(format!("{label} failed (exit {code})")));
    }
    Ok(())
}
