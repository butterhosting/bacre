use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::models::archives::{Backend, Staged, iso};

/// A download in progress lives under this suffix and never counts as staged
pub const PARTIAL: &str = ".partial";

pub fn dir(config: &Config, service: &str, handle: &str) -> Result<PathBuf, String> {
    Ok(config.restic()?.staging_dir.join(service).join(handle))
}

pub async fn list(config: &Config) -> Vec<Staged> {
    let Ok(restic) = config.restic() else {
        return Vec::new();
    };
    let root = &restic.staging_dir;
    let mut staged = Vec::new();
    for service in names(root).await {
        for handle in names(&root.join(&service)).await {
            if handle.ends_with(PARTIAL) {
                continue;
            }
            let path = root.join(&service).join(&handle);
            let Ok(metadata) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if !metadata.is_dir() {
                continue;
            }
            let modified: DateTime<Utc> = metadata
                .modified()
                .map(DateTime::from)
                .unwrap_or_else(|_| Utc::now());
            staged.push(Staged {
                service: service.clone(),
                backend: Backend::Restic,
                handle,
                path: path.to_string_lossy().into_owned(),
                downloaded_at: iso(modified),
            });
        }
    }
    staged.sort_by(|a, b| b.downloaded_at.cmp(&a.downloaded_at));
    staged
}

pub async fn discard(config: &Config, service: &str, handle: &str) -> std::io::Result<()> {
    let Ok(path) = dir(config, service, handle) else {
        return Ok(());
    };
    match tokio::fs::remove_dir_all(path).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

async fn names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::testing;

    #[tokio::test]
    async fn should_list_what_is_on_disk_and_forget_what_is_discarded() {
        let root = tempfile::tempdir().unwrap();
        let mut config = testing::config("");
        config.backends.restic.as_mut().unwrap().staging_dir = root.path().to_path_buf();
        std::fs::create_dir_all(root.path().join("wiki/573591ae")).unwrap();
        std::fs::create_dir_all(root.path().join("wiki/aaaa1111.partial")).unwrap();
        std::fs::write(root.path().join("wiki/stray-file"), "x").unwrap();

        let staged = list(&config).await;
        assert_eq!(staged.len(), 1);
        assert_eq!(
            (
                staged[0].service.as_str(),
                staged[0].handle.as_str(),
                staged[0].backend
            ),
            ("wiki", "573591ae", Backend::Restic)
        );
        assert_eq!(
            staged[0].path,
            root.path().join("wiki/573591ae").to_string_lossy()
        );

        discard(&config, "wiki", "573591ae").await.unwrap();
        discard(&config, "wiki", "573591ae").await.unwrap();
        assert_eq!(list(&config).await, vec![]);
    }

    #[tokio::test]
    async fn should_have_nothing_staged_when_the_directory_does_not_exist() {
        assert_eq!(list(&testing::config("")).await, vec![]);
    }
}
