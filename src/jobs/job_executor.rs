//! Turns a job request into backend calls.

use std::sync::Arc;

use super::job_service::Executor;
use super::log::Log;
use crate::backends::{self, restic_restore};
use crate::failure::{Failure, Outcome};
use crate::models::atlas::Entry;
use crate::models::jobs::Request;
use crate::services::archive_service::ArchiveService;

pub struct JobExecutor {
    archive_service: Arc<ArchiveService>,
}

impl JobExecutor {
    pub fn new(archive_service: Arc<ArchiveService>) -> Arc<Self> {
        Arc::new(Self { archive_service })
    }

    /// In the form the job service takes
    pub fn executor(self: &Arc<Self>) -> Executor {
        let this = self.clone();
        Arc::new(move |request, log| {
            let this = this.clone();
            Box::pin(async move { this.execute(&request, &log).await })
        })
    }

    async fn execute(&self, request: &Request, log: &Log) -> Outcome {
        let ctx = self.archive_service.context();
        match request {
            Request::Backup(backup) => {
                // every target has to be in the atlas before any of them is touched
                let known = request
                    .services()
                    .into_iter()
                    .try_for_each(|service| self.entry(service).map(drop));
                let outcome = match known {
                    Ok(()) => {
                        backends::backup(ctx, backup, &|service| self.entry(service), log).await
                    }
                    unknown => unknown,
                };
                // only a backup changes what the archives hold, and a failed one may have added to them too
                log.info("==> Refreshing the listings");
                self.archive_service.refresh().await;
                outcome
            }
            Request::Download(request) => {
                restic_restore::download(ctx, &self.entry(&request.service)?, &request.handle, log)
                    .await
            }
            Request::Restore(request) => {
                backends::restore(ctx, &self.entry(&request.service)?, request, log).await
            }
        }
    }

    fn entry(&self, service: &str) -> Outcome<Entry> {
        self.archive_service.entry(service).ok_or_else(|| {
            Failure::new(format!(
                "{service} is not declared by any bacre.yaml in the atlas"
            ))
        })
    }
}
