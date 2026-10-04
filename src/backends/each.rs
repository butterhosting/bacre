use std::future::Future;

use crate::failure::{Failure, Outcome};
use crate::jobs::log::Log;

/// Runs `one` for every target. A target that fails is reported and the rest still run, so
/// one broken service does not cost the others their nightly backup; the job fails at the
/// end, naming them.
pub async fn each<'a, T, F>(
    targets: &'a [T],
    service: impl Fn(&T) -> &str,
    log: &Log,
    one: impl Fn(&'a T) -> F,
) -> Outcome
where
    F: Future<Output = Outcome>,
{
    let mut failed = Vec::new();
    for target in targets {
        if let Err(failure) = one(target).await {
            let name = service(target).to_string();
            if targets.len() == 1 {
                return Err(Failure::of(failure.message, vec![name]));
            }
            log.info(format!("==> {name} failed: {}", failure.message));
            failed.push(name);
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    Err(Failure::of(
        format!(
            "{} of {} failed: {}",
            failed.len(),
            targets.len(),
            failed.join(", ")
        ),
        failed,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn log() -> (Log, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        (
            Log::new(move |_, text| sink.lock().unwrap().push(text)),
            lines,
        )
    }

    const TARGETS: [&str; 3] = ["a", "b", "c"];

    #[tokio::test]
    async fn should_run_every_target_when_none_fails() {
        let ran = Mutex::new(Vec::new());
        let result = each(
            &TARGETS,
            |t| t,
            &log().0,
            |target| {
                ran.lock().unwrap().push(*target);
                async { Ok(()) }
            },
        )
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(*ran.lock().unwrap(), vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn should_carry_on_past_a_failing_target_and_fail_at_the_end_naming_it() {
        let ran = Mutex::new(Vec::new());
        let (log, lines) = log();
        let result = each(
            &TARGETS,
            |t| t,
            &log,
            |target| {
                ran.lock().unwrap().push(*target);
                async move {
                    if *target == "b" {
                        Err(Failure::new("no space left"))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;

        assert_eq!(
            result,
            Err(Failure::of("1 of 3 failed: b", vec!["b".to_string()]))
        );
        assert_eq!(*ran.lock().unwrap(), vec!["a", "b", "c"]);
        assert_eq!(*lines.lock().unwrap(), vec!["==> b failed: no space left"]);
    }

    #[tokio::test]
    async fn should_pass_the_error_of_a_lone_target_through_as_it_is() {
        let result = each(
            &["a"],
            |t| t,
            &log().0,
            |_| async { Err(Failure::new("exit 1")) },
        )
        .await;
        assert_eq!(result, Err(Failure::of("exit 1", vec!["a".to_string()])));
    }
}
