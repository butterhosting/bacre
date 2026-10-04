use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use crate::config::Config;
use crate::models::atlas::{self, Entry, Problem};

pub struct AtlasService {
    config: Arc<Config>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scan {
    pub entries: Vec<Entry>,
    pub problems: Vec<Problem>,
}

impl AtlasService {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    pub async fn scan(&self) -> Scan {
        let mut scan = Scan::default();
        let mut seen: BTreeMap<String, String> = BTreeMap::new(); // service → path that declared it first

        for path in self.locate() {
            let problem = |message: String| Problem {
                path: path.clone(),
                message,
            };
            let text = match tokio::fs::read_to_string(&path).await {
                Ok(text) => text,
                Err(e) => {
                    scan.problems
                        .push(problem(format!("could not be read: {e}")));
                    continue;
                }
            };
            let document = match serde_yaml_ng::from_str(&text) {
                Ok(document) => document,
                Err(e) => {
                    scan.problems.push(problem(format!("not valid YAML: {e}")));
                    continue;
                }
            };
            let config = match atlas::parse(&document) {
                Ok(config) => config,
                Err(issues) => {
                    scan.problems
                        .push(problem(format!("invalid config: {}", issues.join("; "))));
                    continue;
                }
            };
            if config.restic.is_some() && self.config.backends.restic.is_none() {
                scan.problems.push(problem(
                    "restic: this daemon has no restic set up (backends.restic in its config)"
                        .to_string(),
                ));
                continue;
            }
            if let Some(restic) = &config.restic
                && !self.config.envsets.contains_key(&restic.envset)
            {
                scan.problems.push(problem(format!(
                    "restic.envset: \"{}\" is not defined in the daemon config",
                    restic.envset
                )));
                continue;
            }
            if let Some(earlier) = seen.get(&config.service) {
                scan.problems.push(problem(format!(
                    "service \"{}\" is already declared by {earlier}",
                    config.service
                )));
                continue;
            }
            seen.insert(config.service.clone(), path.clone());
            scan.entries.push(Entry { path, config });
        }

        scan.entries
            .sort_by(|a, b| a.config.service.cmp(&b.config.service));
        scan
    }

    fn locate(&self) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for pattern in &self.config.services {
            if pattern.contains(['*', '?', '[', '{']) {
                for pattern in expand_braces(pattern) {
                    for path in glob::glob(&pattern).into_iter().flatten().flatten() {
                        if path.is_file() {
                            found.insert(path.to_string_lossy().into_owned());
                        }
                    }
                }
            } else if Path::new(pattern).is_file() {
                found.insert(pattern.clone());
            }
        }
        found
    }
}

/// `a/{b,c}/d` → `a/b/d`, `a/c/d`; nested groups too. A brace without its partner stays.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let mut depth = 0;
    let mut commas = Vec::new();
    for (index, c) in pattern[open..].char_indices() {
        match c {
            '{' => depth += 1,
            ',' if depth == 1 => commas.push(open + index),
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let close = open + index;
                    let mut bounds = vec![open];
                    bounds.extend(commas);
                    bounds.push(close);
                    return bounds
                        .windows(2)
                        .flat_map(|pair| {
                            expand_braces(&format!(
                                "{}{}{}",
                                &pattern[..open],
                                &pattern[pair[0] + 1..pair[1]],
                                &pattern[close + 1..]
                            ))
                        })
                        .collect();
                }
            }
            _ => {}
        }
    }
    vec![pattern.to_string()]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::testing;

    fn valid(service: &str) -> String {
        format!(
            "service: {service}\nhome: /opt/{service}\nbtrfs:\n  subvolume: /live/@{service}\n  destinations: [/live/.snapshots]\n  retention:\n    keepLast: 3\n"
        )
    }

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn root(&self) -> String {
            // the canonical path: the patterns are matched against what is really on disk
            self.dir
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        }

        fn write(&self, relative: &str, content: &str) -> String {
            let path = PathBuf::from(self.root()).join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path.to_string_lossy().into_owned()
        }
    }

    fn atlas(patterns: &[String], envsets: &str) -> AtlasService {
        let services: Vec<String> = patterns
            .iter()
            .map(|pattern| format!("'{pattern}'"))
            .collect();
        AtlasService::new(Arc::new(testing::config(&format!(
            "services: [{}]\nenvsets: {envsets}",
            services.join(", ")
        ))))
    }

    fn services(scan: &Scan) -> Vec<&str> {
        scan.entries
            .iter()
            .map(|entry| entry.config.service.as_str())
            .collect()
    }

    #[tokio::test]
    async fn should_find_nothing_when_the_atlas_is_empty() {
        assert_eq!(atlas(&[], "{}").scan().await, Scan::default());
    }

    #[tokio::test]
    async fn should_expand_a_glob_to_exactly_the_files_it_matches() {
        let fixture = Fixture::new();
        fixture.write("a/bacre.yaml", &valid("alpha"));
        fixture.write("b/bacre.yaml", &valid("beta"));
        fixture.write("b/notes.yaml", &valid("ignored"));
        fixture.write("c/bacre.yml", &valid("also-ignored"));

        let scan = atlas(&[format!("{}/*/bacre.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(scan.problems, vec![]);
        assert_eq!(services(&scan), vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn should_take_whatever_file_names_the_operator_points_at() {
        let fixture = Fixture::new();
        fixture.write("services/nextcloud.yaml", &valid("nextcloud"));
        fixture.write("extra/anything.yml", &valid("extra"));

        let scan = atlas(
            &[
                format!("{}/services/*.yaml", fixture.root()),
                format!("{}/extra/*.yml", fixture.root()),
            ],
            "{}",
        )
        .scan()
        .await;

        assert_eq!(scan.problems, vec![]);
        assert_eq!(services(&scan), vec!["extra", "nextcloud"]);
    }

    #[tokio::test]
    async fn should_expand_braces_in_a_pattern() {
        let fixture = Fixture::new();
        fixture.write("a/bacre.yaml", &valid("alpha"));
        fixture.write("b/bacre.yaml", &valid("beta"));
        fixture.write("c/bacre.yaml", &valid("gamma"));

        let scan = atlas(&[format!("{}/{{a,c}}/bacre.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(services(&scan), vec!["alpha", "gamma"]);
        assert_eq!(expand_braces("x/{a,b{1,2}}/y.{yaml,yml}").len(), 6);
        assert_eq!(expand_braces("x/{a/y"), vec!["x/{a/y"]);
    }

    #[tokio::test]
    async fn should_also_accept_direct_file_paths_and_ignore_missing_ones() {
        let fixture = Fixture::new();
        let path = fixture.write("solo.yaml", &valid("solo"));

        let scan = atlas(&[path, format!("{}/missing.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(scan.problems, vec![]);
        assert_eq!(services(&scan), vec!["solo"]);
    }

    #[tokio::test]
    async fn should_report_an_invalid_config_as_a_problem_and_keep_the_others() {
        let fixture = Fixture::new();
        fixture.write("ok/bacre.yaml", &valid("ok"));
        let bad = fixture.write(
            "bad/bacre.yaml",
            "service: bad\nbtrfs:\n  subvolume: nope\n",
        );

        let scan = atlas(&[format!("{}/*/bacre.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(services(&scan), vec!["ok"]);
        assert_eq!(scan.problems.len(), 1);
        assert_eq!(scan.problems[0].path, bad);
        assert!(
            scan.problems[0].message.contains("home"),
            "{}",
            scan.problems[0].message
        );
        assert!(
            scan.problems[0].message.contains("btrfs.subvolume"),
            "{}",
            scan.problems[0].message
        );
    }

    #[tokio::test]
    async fn should_report_unparseable_yaml_as_a_problem() {
        let fixture = Fixture::new();
        fixture.write("x/bacre.yaml", "service: [unclosed\n");

        let scan = atlas(&[format!("{}/*/bacre.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(scan.entries, vec![]);
        assert!(scan.problems[0].message.starts_with("not valid YAML"));
    }

    #[tokio::test]
    async fn should_refuse_a_second_file_declaring_the_same_service() {
        let fixture = Fixture::new();
        fixture.write("a/bacre.yaml", &valid("twin"));
        fixture.write("b/bacre.yaml", &valid("twin"));

        let scan = atlas(&[format!("{}/*/bacre.yaml", fixture.root())], "{}")
            .scan()
            .await;

        assert_eq!(scan.entries.len(), 1);
        assert!(scan.problems[0].message.contains("already declared"));
    }

    #[tokio::test]
    async fn should_refuse_a_restic_envset_the_daemon_config_does_not_define() {
        let fixture = Fixture::new();
        fixture.write(
            "r/bacre.yaml",
            "service: r\nhome: /opt/r\nrestic:\n  repository: s3:x/r\n  envset: known\n  retention: { keepLast: 1, keepDaily: 1, keepWeekly: 1, keepMonthly: 1 }\n  backupPaths: [/opt/r]\n",
        );
        let pattern = [format!("{}/*/bacre.yaml", fixture.root())];

        assert_eq!(
            atlas(&pattern, "{ known: { RESTIC_PASSWORD: x } }")
                .scan()
                .await
                .entries
                .len(),
            1
        );
        let scan = atlas(&pattern, "{}").scan().await;
        assert_eq!(scan.entries.len(), 0);
        assert!(
            scan.problems[0]
                .message
                .contains("\"known\" is not defined")
        );
    }

    #[tokio::test]
    async fn should_refuse_restic_on_a_daemon_without_restic_but_keep_btrfs_only_services() {
        let fixture = Fixture::new();
        fixture.write("a/bacre.yaml", &valid("local"));
        fixture.write(
            "b/bacre.yaml",
            "service: offsite\nhome: /opt/o\nrestic:\n  repository: s3:x/o\n  envset: e\n  retention: { keepLast: 1, keepDaily: 1, keepWeekly: 1, keepMonthly: 1 }\n  backupPaths: [/opt/o]\n",
        );
        let mut config = testing::config(&format!(
            "services: ['{}/*/bacre.yaml']\nenvsets: {{ e: {{}} }}",
            fixture.root()
        ));
        config.backends.restic = None;

        let scan = AtlasService::new(Arc::new(config)).scan().await;

        assert_eq!(services(&scan), vec!["local"]);
        assert!(
            scan.problems[0]
                .message
                .starts_with("restic: this daemon has no restic set up")
        );
    }

    #[tokio::test]
    async fn should_read_every_part_of_a_full_bacre_yaml() {
        let fixture = Fixture::new();
        fixture.write(
            "wiki.yaml",
            r#"
service: wiki
home: /srv/services/wiki
btrfs:
  schedule: " 5 * * * * "
  subvolume: /srv/disk-a/@wiki
  destinations: [/srv/disk-a/.snapshots, /srv/disk-b/.snapshots]
  retention: { keepLast: 6, keepHourly: 24, keepDaily: 7 }
  lifecycle:
    stop: docker compose down
    start: |
      docker compose up --wait
restic:
  schedule: "0 3 * * *"
  repository: s3:example/wiki
  envset: demo
  retention: { keepLast: 3, keepHourly: 0, keepDaily: 30, keepWeekly: 15, keepMonthly: 12 }
  backupPaths: [/srv/disk-a/@wiki/data]
  lifecycle:
    backupPrepare: docker compose stop main
"#,
        );

        let scan = atlas(&[format!("{}/wiki.yaml", fixture.root())], "{ demo: {} }")
            .scan()
            .await;
        assert_eq!(scan.problems, vec![]);
        let config = &scan.entries[0].config;
        let btrfs = config.btrfs.as_ref().unwrap();
        let restic = config.restic.as_ref().unwrap();

        assert_eq!(btrfs.schedule.as_deref(), Some("5 * * * *"));
        assert_eq!(
            btrfs.lifecycle.as_ref().unwrap().start,
            "docker compose up --wait"
        );
        assert_eq!(
            btrfs.destinations,
            vec!["/srv/disk-a/.snapshots", "/srv/disk-b/.snapshots"]
        );
        assert_eq!(
            (btrfs.retention.keep_hourly, btrfs.retention.keep_weekly),
            (24, 0)
        );
        assert_eq!(restic.retention.keep_weekly, 15);
        assert_eq!(
            restic.lifecycle.backup_prepare.as_deref(),
            Some("docker compose stop main")
        );
        assert_eq!(restic.lifecycle.restore_apply, None);
    }

    #[tokio::test]
    async fn should_name_every_rule_a_bacre_yaml_breaks() {
        let fixture = Fixture::new();
        fixture.write(
            "bad.yaml",
            "service: Bad Name\nhome: relative\nbtrfs:\n  subvolume: /live/@x\n  snapshots: /elsewhere/.snapshots\n  destinations: [/live/.snapshots, /live/@x/.snapshots, /live/.snapshots]\n  retention: { preserveMin: 24h }\n  schedule: daily\nrestic:\n  repository: r\n  envset: e\n  retention: { keepLast: -1, keepDaily: 1, keepWeekly: 1, keepMonthly: 1 }\n  backupPaths: [relative]\n  lifecycle: { restoreApply: '  ' }\n",
        );

        let scan = atlas(&[format!("{}/bad.yaml", fixture.root())], "{ e: {} }")
            .scan()
            .await;
        let message = &scan.problems[0].message;

        for part in [
            "service: ",
            "home: ",
            "btrfs.snapshots: replaced by destinations",
            "btrfs.destinations.1: must not be inside the subvolume",
            "btrfs.destinations.2: is listed twice",
            "btrfs.retention.preserveMin: btrbk's retention is gone",
            "btrfs.retention.keepLast: keeps nothing",
            "btrfs.schedule: \"daily\" is not a cron expression",
            "restic.retention.keepLast: ",
            "restic.backupPaths.0: ",
            "restic.lifecycle.restoreApply: ",
        ] {
            assert!(message.contains(part), "missing {part:?} in: {message}");
        }
    }
}
