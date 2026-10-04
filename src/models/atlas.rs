//! A file is read in one pass that collects every problem it has, rather than stopping at the first.

use serde_yaml_ng::{Mapping, Value};

use crate::cron::Cron;
use crate::models::archives::Backend;

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub service: String,
    pub home: String,
    pub btrfs: Option<BtrfsConfig>,
    pub restic: Option<ResticConfig>,
}

impl ServiceConfig {
    pub fn configures(&self, backend: Backend) -> bool {
        match backend {
            Backend::Btrfs => self.btrfs.is_some(),
            Backend::Restic => self.restic.is_some(),
        }
    }

    pub fn schedule(&self, backend: Backend) -> Option<&str> {
        match backend {
            Backend::Btrfs => self.btrfs.as_ref()?.schedule.as_deref(),
            Backend::Restic => self.restic.as_ref()?.schedule.as_deref(),
        }
    }
}

/// In btrbk's words: the subvolume's directory is the `volume`, `snapshots` is its
/// `snapshot_dir` (same filesystem, by btrfs's rules), and every entry of `targets` is a
/// `target` that receives a copy of each snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct BtrfsConfig {
    pub subvolume: String,
    pub snapshots: String,
    pub targets: Vec<String>,
    pub retention: BtrfsRetention,
    pub schedule: Option<String>,
    pub lifecycle: Option<BtrfsLifecycle>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BtrfsRetention {
    pub preserve_min: String,
    pub preserve: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BtrfsLifecycle {
    pub stop: String,
    pub start: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResticConfig {
    pub repository: String,
    pub envset: String,
    pub retention: ResticRetention,
    pub schedule: Option<String>,
    pub backup_paths: Vec<String>,
    pub lifecycle: ResticLifecycle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResticRetention {
    pub keep_last: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResticLifecycle {
    pub backup_prepare: Option<String>,
    pub backup_release: Option<String>,
    /// `$BACRE_STAGING` holds the download, with the backed-up absolute paths underneath it
    pub restore_apply: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    pub config: ServiceConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

pub fn parse(document: &Value) -> Result<ServiceConfig, Vec<String>> {
    let mut reader = Reader::default();
    let Some(root) = document.as_mapping() else {
        return Err(vec!["(root): expected a mapping".to_string()]);
    };

    let service = reader.string(root, "service").filter(|service| {
        let valid = is_service_name(service);
        if !valid {
            reader.issue("service", "a lowercase service name");
        }
        valid
    });
    let home = reader.absolute(root, "home");
    let btrfs = reader.block(root, "btrfs", btrfs);
    let restic = reader.block(root, "restic", restic);

    match (service, home, reader.issues.is_empty()) {
        (Some(service), Some(home), true) => Ok(ServiceConfig {
            service,
            home,
            btrfs: btrfs.flatten(),
            restic: restic.flatten(),
        }),
        _ => Err(reader.issues),
    }
}

pub fn is_service_name(name: &str) -> bool {
    let mut chars = name.chars();
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.next().is_some_and(allowed) && chars.all(|c| allowed(c) || c == '_' || c == '-')
}

fn btrfs(reader: &mut Reader, map: &Mapping) -> Option<BtrfsConfig> {
    let subvolume = reader.string(map, "subvolume").filter(|subvolume| {
        let name = subvolume.rsplit('/').next().unwrap_or_default();
        let valid = subvolume.starts_with('/')
            && subvolume.matches('/').count() >= 2
            && name.len() > 1
            && name.starts_with('@')
            && !name.chars().any(char::is_whitespace);
        if !valid {
            reader.issue(
                "subvolume",
                "an absolute path to a subvolume named like @wiki",
            );
        }
        valid
    });
    let snapshots = reader.absolute(map, "snapshots");
    let targets = reader.absolutes(map, "targets", 0);
    let retention = reader.block(map, "retention", |reader, map| {
        let preserve_min = reader
            .string(map, "preserveMin")
            .filter(|text| reader.non_empty("preserveMin", text));
        let preserve = reader.strings(map, "preserve", 1);
        Some(BtrfsRetention {
            preserve_min: preserve_min?,
            preserve: preserve?,
        })
    });
    if retention.is_none() {
        reader.issue("retention", "expected a mapping");
    }
    let schedule = reader.schedule(map);
    let lifecycle = reader.block(map, "lifecycle", |reader, map| {
        let stop = reader.hook(map, "stop", true);
        let start = reader.hook(map, "start", true);
        Some(BtrfsLifecycle {
            stop: stop?,
            start: start?,
        })
    });

    if let (Some(subvolume), Some(snapshots)) = (&subvolume, &snapshots) {
        let parent = &subvolume[..subvolume.rfind('/').unwrap_or(0)];
        if !snapshots.starts_with(&format!("{parent}/")) {
            reader.issue(
                "snapshots",
                "must be on the subvolume's filesystem (inside the subvolume's parent directory)",
            );
        }
    }

    Some(BtrfsConfig {
        subvolume: subvolume?,
        snapshots: snapshots?,
        targets: targets?,
        retention: retention??,
        schedule,
        lifecycle: lifecycle.flatten(),
    })
}

fn restic(reader: &mut Reader, map: &Mapping) -> Option<ResticConfig> {
    let repository = reader
        .string(map, "repository")
        .filter(|text| reader.non_empty("repository", text));
    let envset = reader
        .string(map, "envset")
        .filter(|text| reader.non_empty("envset", text));
    let retention = reader.block(map, "retention", |reader, map| {
        let keep_last = reader.count(map, "keepLast");
        let keep_daily = reader.count(map, "keepDaily");
        let keep_weekly = reader.count(map, "keepWeekly");
        let keep_monthly = reader.count(map, "keepMonthly");
        Some(ResticRetention {
            keep_last: keep_last?,
            keep_daily: keep_daily?,
            keep_weekly: keep_weekly?,
            keep_monthly: keep_monthly?,
        })
    });
    if retention.is_none() {
        reader.issue("retention", "expected a mapping");
    }
    let schedule = reader.schedule(map);
    let backup_paths = reader.absolutes(map, "backupPaths", 1);
    let lifecycle = reader.block(map, "lifecycle", |reader, map| {
        Some(ResticLifecycle {
            backup_prepare: reader.hook(map, "backupPrepare", false),
            backup_release: reader.hook(map, "backupRelease", false),
            restore_apply: reader.hook(map, "restoreApply", false),
        })
    });

    Some(ResticConfig {
        repository: repository?,
        envset: envset?,
        retention: retention??,
        schedule,
        backup_paths: backup_paths?,
        lifecycle: lifecycle.flatten().unwrap_or_default(),
    })
}

#[derive(Default)]
struct Reader {
    path: Vec<String>,
    issues: Vec<String>,
}

impl Reader {
    fn issue(&mut self, key: &str, message: &str) {
        let mut path = self.path.clone();
        path.push(key.to_string());
        self.issues.push(format!("{}: {message}", path.join(".")));
    }

    /// The value under a key; an explicit null counts as not there
    fn get<'a>(&self, map: &'a Mapping, key: &str) -> Option<&'a Value> {
        map.get(key).filter(|value| !value.is_null())
    }

    /// The outer `None`: the key is absent. `Some(None)`: there, but wrong (and reported).
    fn block<T>(
        &mut self,
        map: &Mapping,
        key: &str,
        read: impl FnOnce(&mut Reader, &Mapping) -> Option<T>,
    ) -> Option<Option<T>> {
        let value = self.get(map, key)?;
        let Some(inner) = value.as_mapping() else {
            self.issue(key, "expected a mapping");
            return Some(None);
        };
        self.path.push(key.to_string());
        let result = read(self, inner);
        self.path.pop();
        Some(result)
    }

    fn string(&mut self, map: &Mapping, key: &str) -> Option<String> {
        match self.get(map, key) {
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => {
                self.issue(key, "expected a string");
                None
            }
            None => {
                self.issue(key, "expected a string, found nothing");
                None
            }
        }
    }

    fn non_empty(&mut self, key: &str, text: &str) -> bool {
        if text.is_empty() {
            self.issue(key, "must not be empty");
        }
        !text.is_empty()
    }

    fn absolute(&mut self, map: &Mapping, key: &str) -> Option<String> {
        self.string(map, key).filter(|path| {
            if !path.starts_with('/') {
                self.issue(key, "must be an absolute path");
            }
            path.starts_with('/')
        })
    }

    fn strings(&mut self, map: &Mapping, key: &str, at_least: usize) -> Option<Vec<String>> {
        let items = match self.get(map, key) {
            None => Vec::new(),
            Some(Value::Sequence(items)) => items.clone(),
            Some(_) => {
                self.issue(key, "expected a list");
                return None;
            }
        };
        let mut texts = Vec::new();
        for (index, item) in items.iter().enumerate() {
            match item {
                Value::String(text) if !text.is_empty() => texts.push(text.clone()),
                _ => self.issue(&format!("{key}.{index}"), "expected a non-empty string"),
            }
        }
        if texts.len() != items.len() {
            return None;
        }
        if texts.len() < at_least {
            self.issue(key, &format!("needs at least {at_least}"));
            return None;
        }
        Some(texts)
    }

    fn absolutes(&mut self, map: &Mapping, key: &str, at_least: usize) -> Option<Vec<String>> {
        let paths = self.strings(map, key, at_least)?;
        let mut valid = true;
        for (index, path) in paths.iter().enumerate() {
            if !path.starts_with('/') {
                self.issue(&format!("{key}.{index}"), "must be an absolute path");
                valid = false;
            }
        }
        valid.then_some(paths)
    }

    fn count(&mut self, map: &Mapping, key: &str) -> Option<u32> {
        let number = self
            .get(map, key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());
        if number.is_none() {
            self.issue(key, "expected a whole number, zero or more");
        }
        number
    }

    fn hook(&mut self, map: &Mapping, key: &str, required: bool) -> Option<String> {
        if !required && self.get(map, key).is_none() {
            return None;
        }
        let script = self.string(map, key)?.trim().to_string();
        self.non_empty(key, &script).then_some(script)
    }

    fn schedule(&mut self, map: &Mapping) -> Option<String> {
        self.get(map, "schedule")?;
        let expression = self.string(map, "schedule")?.trim().to_string();
        match Cron::parse(&expression) {
            Ok(_) => Some(expression),
            Err(message) => {
                self.issue("schedule", &message);
                None
            }
        }
    }
}
