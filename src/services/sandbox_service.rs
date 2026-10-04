//! The seeded world, one service per thing worth seeing on the website:
//!
//! | service | btrfs                     | restic                                         |
//! |---------|---------------------------|------------------------------------------------|
//! | wiki    | hourly, lifecycle         | daily, recent history, database, restorable    |
//! | mailbox | hourly, lifecycle         | daily, recent history, no database, restorable |
//! | tracker | hourly, lifecycle         | daily, recent history, no restore hook         |
//! | ledger  | hourly, lifecycle         | by hand only, history that stopped 9 days ago  |
//! | gallery | hourly, lifecycle         | daily, but its envset has the wrong password   |
//! | recipes | hourly, lifecycle         | daily, no repository yet                       |
//! | dns     | hourly, lifecycle         | —                                              |
//! | radio   | by hand, no lifecycle     | —                                              |

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Local, TimeZone};
use futures_util::future::try_join_all;

use crate::backends::Context;
use crate::jobs::job_service::{Busy, Hold, JobService};
use crate::services::archive_service::ArchiveService;
use crate::services::scheduler::Scheduler;
use crate::shell::Env;

const ENVSET: &str = "demo";
const WRONG_ENVSET: &str = "demo-wrong-password";

pub struct SandboxService {
    root: PathBuf,
    ctx: Context,
    job_service: Arc<JobService>,
    scheduler: Arc<Scheduler>,
    archive_service: Arc<ArchiveService>,
}

#[derive(Debug)]
pub enum Refusal {
    Busy(Busy),
    Failed(String),
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Refusal::Failed(message)
    }
}

impl SandboxService {
    pub fn new(
        ctx: Context,
        job_service: Arc<JobService>,
        scheduler: Arc<Scheduler>,
        archive_service: Arc<ArchiveService>,
    ) -> Option<Arc<Self>> {
        let root = ctx.config.sandbox.clone()?;
        Some(Arc::new(Self {
            root,
            ctx,
            job_service,
            scheduler,
            archive_service,
        }))
    }

    pub async fn purge(&self) -> Result<(), Refusal> {
        let hold = self.job_service.hold().map_err(Refusal::Busy)?;
        self.empty().await?;
        self.forget(&hold).await;
        Ok(())
    }

    pub async fn seed(&self) -> Result<(), Refusal> {
        let hold = self.job_service.hold().map_err(Refusal::Busy)?;
        self.empty().await?;
        let built = self.build().await;
        self.forget(&hold).await;
        built.map_err(Refusal::Failed)
    }

    pub async fn seed_if_empty(&self) {
        if tokio::fs::metadata(self.root.join("services"))
            .await
            .is_ok()
        {
            return;
        }
        println!("==> Seeding the sandbox at {}", self.root.display());
        match self.seed().await {
            Ok(()) => println!("==> Seeded"),
            Err(refusal) => eprintln!("==> The sandbox could not be seeded: {refusal:?}"),
        }
    }

    /// Everything in the sandbox goes, the directory itself stays: it may be a mount point
    async fn empty(&self) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("could not empty {}: {e}", self.root.display());
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(failed)?;
        let mut entries = tokio::fs::read_dir(&self.root).await.map_err(failed)?;
        while let Some(entry) = entries.next_entry().await.map_err(failed)? {
            let path = entry.path();
            if entry.file_type().await.map_err(failed)?.is_dir() {
                tokio::fs::remove_dir_all(&path).await.map_err(failed)?;
            } else {
                tokio::fs::remove_file(&path).await.map_err(failed)?;
            }
        }
        Ok(())
    }

    async fn forget(&self, hold: &Hold) {
        self.job_service.forget_all(hold);
        self.scheduler.forget_waiting();
        self.archive_service.refresh().await;
    }

    async fn build(&self) -> Result<(), String> {
        for envset in [ENVSET, WRONG_ENVSET] {
            if !self.ctx.config.envsets.contains_key(envset) {
                return Err(format!(
                    "the config needs the envsets {ENVSET} and {WRONG_ENVSET} for the sandbox (see config.dev.yaml)"
                ));
            }
        }
        let world = World::new(&self.root);
        for dir in [&world.snapshots, &world.target] {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| e.to_string())?;
        }
        let now = Local::now();
        try_join_all(
            SPECS
                .iter()
                .map(|spec| self.build_service(&world, spec, now)),
        )
        .await?;
        Ok(())
    }

    /// Files first, then the repository and its history; the bacre.yaml last, so the atlas
    /// never sees a service that is only half there
    async fn build_service(
        &self,
        world: &World,
        spec: &Spec,
        now: DateTime<Local>,
    ) -> Result<(), String> {
        let paths = world.paths(spec.name);
        for (path, content) in files(spec, &paths) {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }

        if let Some(restic) = &spec.restic {
            let moments = restic.history.moments(now);
            if !moments.is_empty() {
                self.restic(&paths, &["init"]).await?;
                if spec.database {
                    tokio::fs::copy(&paths.database, &paths.dump)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                for moment in moments {
                    let time = moment.format("%Y-%m-%d %H:%M:%S").to_string();
                    let tag = format!("t={}", moment.format("%Y%m%dT%H%M"));
                    let mut args = vec!["backup", "--time", &time, "--tag", &tag];
                    let backup_paths = paths.backup_paths(spec);
                    args.extend(backup_paths.iter().map(String::as_str));
                    self.restic(&paths, &args).await?;
                }
                if spec.database {
                    tokio::fs::remove_file(&paths.dump)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        }

        let yaml = bacre_yaml(world, spec);
        tokio::fs::write(paths.home.join("bacre.yaml"), yaml)
            .await
            .map_err(|e| e.to_string())
    }

    async fn restic(&self, paths: &Paths, args: &[&str]) -> Result<(), String> {
        let mut command = vec![
            "restic".to_string(),
            "-r".to_string(),
            paths.repository.to_string_lossy().into_owned(),
            "--cache-dir".to_string(),
            self.ctx
                .config
                .restic()?
                .cache_dir
                .to_string_lossy()
                .into_owned(),
        ];
        command.extend(args.iter().map(|arg| arg.to_string()));
        let env: Env = self.ctx.config.envsets[ENVSET].clone();
        let output = self.ctx.shell.run(&command, &env).await;
        if output.code != 0 {
            return Err(format!(
                "restic {} failed for {}: {}",
                args[0],
                paths.repository.display(),
                output.stderr.trim()
            ));
        }
        Ok(())
    }
}

struct Spec {
    name: &'static str,
    database: bool,
    lifecycle: bool,
    restic: Option<ResticSpec>,
}

struct ResticSpec {
    envset: &'static str,
    scheduled: bool,
    history: History,
    hooks: bool,
    restorable: bool,
}

#[derive(Clone, Copy)]
enum History {
    Nothing,
    Recent,
    Stale,
}

impl History {
    fn moments(self, now: DateTime<Local>) -> Vec<DateTime<Local>> {
        match self {
            History::Nothing => vec![],
            History::Recent => [0, 1, 2, 3]
                .iter()
                .map(|days| now - Duration::days(*days) - Duration::minutes(10))
                .collect(),
            History::Stale => (9..=12)
                .rev()
                .map(|days| at_night(now - Duration::days(days)))
                .collect(),
        }
    }
}

fn at_night(day: DateTime<Local>) -> DateTime<Local> {
    let wall = day.date_naive().and_hms_opt(3, 1, 4).expect("a valid time");
    Local.from_local_datetime(&wall).earliest().unwrap_or(day)
}

const SPECS: [Spec; 8] = [
    Spec {
        name: "dns",
        database: false,
        lifecycle: true,
        restic: None,
    },
    Spec {
        name: "gallery",
        database: true,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: WRONG_ENVSET,
            scheduled: true,
            history: History::Recent,
            hooks: true,
            restorable: true,
        }),
    },
    Spec {
        name: "ledger",
        database: true,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: ENVSET,
            scheduled: false,
            history: History::Stale,
            hooks: true,
            restorable: true,
        }),
    },
    Spec {
        name: "mailbox",
        database: false,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: ENVSET,
            scheduled: true,
            history: History::Recent,
            hooks: true,
            restorable: true,
        }),
    },
    Spec {
        name: "radio",
        database: false,
        lifecycle: false,
        restic: None,
    },
    Spec {
        name: "recipes",
        database: false,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: ENVSET,
            scheduled: true,
            history: History::Nothing,
            hooks: false,
            restorable: false,
        }),
    },
    Spec {
        name: "tracker",
        database: true,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: ENVSET,
            scheduled: true,
            history: History::Recent,
            hooks: true,
            restorable: false,
        }),
    },
    Spec {
        name: "wiki",
        database: true,
        lifecycle: true,
        restic: Some(ResticSpec {
            envset: ENVSET,
            scheduled: true,
            history: History::Recent,
            hooks: true,
            restorable: true,
        }),
    },
];

struct World {
    root: PathBuf,
    snapshots: PathBuf,
    target: PathBuf,
}

struct Paths {
    home: PathBuf,
    live: PathBuf,
    database: PathBuf,
    dumps: PathBuf,
    dump: PathBuf,
    repository: PathBuf,
}

impl World {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            snapshots: root.join("disk-a/.snapshots"),
            target: root.join("disk-b/.snapshots"),
        }
    }

    fn paths(&self, name: &str) -> Paths {
        let live = self.root.join(format!("disk-a/@{name}"));
        let dumps = self.root.join(format!("dumps/{name}"));
        Paths {
            home: self.root.join(format!("services/{name}")),
            database: live.join(format!("db/{name}.db")),
            dump: dumps.join(format!("{name}.sql")),
            live,
            dumps,
            repository: self.root.join(format!("repos/{name}")),
        }
    }
}

impl Paths {
    fn backup_paths(&self, spec: &Spec) -> Vec<String> {
        let mut paths = vec![show(&self.live.join("data"))];
        if spec.database {
            paths.push(show(&self.dumps));
        }
        paths
    }
}

fn show(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn files(spec: &Spec, paths: &Paths) -> Vec<(PathBuf, String)> {
    let name = spec.name;
    let mut files = vec![
        (
            paths.live.join("data/README.md"),
            format!("This is {name}, as seeded.\n"),
        ),
        (
            paths.live.join("data/notes/first.md"),
            format!("The first note of {name}.\n"),
        ),
        (paths.home.join("state"), "running\n".to_string()),
    ];
    if spec.database {
        files.push((
            paths.database.clone(),
            format!("{name}: the database, as seeded.\n"),
        ));
        files.push((paths.dumps.join(".keep"), String::new()));
    }
    files
}

fn bacre_yaml(world: &World, spec: &Spec) -> String {
    let paths = world.paths(spec.name);
    let (name, live, dumps) = (spec.name, show(&paths.live), show(&paths.dumps));
    let mut yaml = format!("service: {name}\nhome: {}\n\nbtrfs:\n", show(&paths.home));
    if spec.lifecycle {
        yaml += "  schedule: \"5 * * * *\"\n";
    }
    yaml += &format!(
        "  subvolume: {live}\n  snapshots: {}\n  targets:\n    - {}\n  retention:\n    preserveMin: 24h\n    preserve: [72h, 30d]\n",
        show(&world.snapshots),
        show(&world.target)
    );
    if spec.lifecycle {
        yaml += "  lifecycle:\n    stop: echo stopped > state\n    start: echo running > state\n";
    }

    let Some(restic) = &spec.restic else {
        return yaml;
    };
    yaml += "\nrestic:\n";
    if restic.scheduled {
        yaml += "  schedule: \"0 3 * * *\"\n";
    }
    yaml += &format!(
        "  repository: {}\n  envset: {}\n  retention:\n    keepLast: 3\n    keepDaily: 30\n    keepWeekly: 15\n    keepMonthly: 12\n  backupPaths:\n",
        show(&paths.repository),
        restic.envset
    );
    for path in paths.backup_paths(spec) {
        yaml += &format!("    - {path}\n");
    }
    if !restic.hooks {
        return yaml;
    }

    yaml += "  lifecycle:\n";
    if spec.database {
        yaml += &format!(
            "    backupPrepare: |\n      echo stopped > state\n      cp {live}/db/{name}.db {dumps}/{name}.sql\n    backupRelease: |\n      rm -f {dumps}/{name}.sql\n      echo running > state\n"
        );
    } else {
        yaml +=
            "    backupPrepare: echo stopped > state\n    backupRelease: echo running > state\n";
    }
    if restic.restorable {
        yaml += &format!(
            "    restoreApply: |\n      echo stopped > state\n      rm -rf {live}/data\n      cp -a \"$BACRE_STAGING{live}/data\" \\\n        {live}/data\n"
        );
        if spec.database {
            yaml += &format!(
                "      cp \"$BACRE_STAGING{dumps}/{name}.sql\" \\\n        {live}/db/{name}.db\n"
            );
        }
        yaml += "      echo running > state\n";
    }
    yaml
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::testing;
    use crate::models::atlas;
    use crate::services::atlas_service::AtlasService;

    #[test]
    fn should_write_a_valid_bacre_yaml_for_every_seeded_service() {
        let world = World::new(Path::new("/app/.sandbox"));
        for spec in &SPECS {
            let yaml = bacre_yaml(&world, spec);
            let document = serde_yaml_ng::from_str(&yaml)
                .unwrap_or_else(|e| panic!("{}: {e}\n{yaml}", spec.name));
            let config = atlas::parse(&document)
                .unwrap_or_else(|issues| panic!("{}: {issues:?}\n{yaml}", spec.name));

            assert_eq!(config.service, spec.name);
            assert_eq!(
                config.btrfs.as_ref().unwrap().lifecycle.is_some(),
                spec.lifecycle
            );
            match (&spec.restic, &config.restic) {
                (None, None) => {}
                (Some(seeded), Some(read)) => {
                    assert_eq!(read.envset, seeded.envset);
                    assert_eq!(read.schedule.is_some(), seeded.scheduled);
                    assert_eq!(
                        read.lifecycle.restore_apply.is_some(),
                        seeded.hooks && seeded.restorable
                    );
                    assert_eq!(read.backup_paths.len(), if spec.database { 2 } else { 1 });
                }
                _ => panic!("{}: restic block does not match the spec", spec.name),
            }
        }
    }

    #[test]
    fn should_restore_a_database_service_from_the_paths_it_backs_up() {
        let world = World::new(Path::new("/app/.sandbox"));
        let wiki = SPECS.iter().find(|spec| spec.name == "wiki").unwrap();
        let yaml = bacre_yaml(&world, wiki);
        assert!(
            yaml.contains(
                "    - /app/.sandbox/disk-a/@wiki/data\n    - /app/.sandbox/dumps/wiki\n"
            ),
            "{yaml}"
        );
        assert!(yaml.contains("cp -a \"$BACRE_STAGING/app/.sandbox/disk-a/@wiki/data\" \\\n        /app/.sandbox/disk-a/@wiki/data\n"), "{yaml}");
        assert!(yaml.contains("cp \"$BACRE_STAGING/app/.sandbox/dumps/wiki/wiki.sql\" \\\n        /app/.sandbox/disk-a/@wiki/db/wiki.db\n"), "{yaml}");
    }

    #[test]
    fn should_seed_recent_history_that_is_newer_than_the_last_scheduled_run() {
        let now = Local::now();
        let recent = History::Recent.moments(now);
        assert_eq!(recent.len(), 4);
        assert!(recent.iter().all(|moment| *moment < now));
        assert!(now - recent[0] < Duration::minutes(11));

        let stale = History::Stale.moments(now);
        assert!(stale.iter().all(|moment| now - *moment > Duration::days(8)));
        assert!(History::Nothing.moments(now).is_empty());
    }

    #[tokio::test]
    async fn should_find_every_seeded_bacre_yaml_through_the_atlas() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let world = World::new(&root);
        for spec in &SPECS {
            let home = world.paths(spec.name).home;
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(home.join("bacre.yaml"), bacre_yaml(&world, spec)).unwrap();
        }
        let config = testing::config(&format!(
            "services: ['{}/services/*/bacre.yaml']\nenvsets: {{ demo: {{}}, demo-wrong-password: {{}} }}",
            root.display()
        ));

        let scan = AtlasService::new(Arc::new(config)).scan().await;

        assert_eq!(scan.problems, vec![]);
        let names: Vec<&str> = scan
            .entries
            .iter()
            .map(|entry| entry.config.service.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "dns", "gallery", "ledger", "mailbox", "radio", "recipes", "tracker", "wiki"
            ]
        );
    }
}
