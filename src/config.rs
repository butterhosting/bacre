//! The daemon's own configuration, one YAML file passed on the command line. Everything a
//! service needs is in its bacre.yaml instead; this only holds what is Bacre's: where to
//! listen, where the services' bacre.yaml files are, who may log in, and the named sets of
//! environment variables those files refer to.
//!
//! Every path in it may be relative, and is then relative to the config file itself, so a
//! config can keep its working directories beside it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// `dev` answers btrfs and btrbk with a fake (a container has no btrfs) and runs everything else
    /// for real against the sandbox
    pub stage: Stage,
    pub server: Server,
    /// Globs or paths of the `bacre.yaml` files, one per service (braces allowed)
    pub services: Vec<String>,
    /// Where short-lived files go (the btrbk configuration of a running backup)
    pub tmp_dir: PathBuf,
    /// Basic auth is on when this names at least one user
    pub users: Vec<User>,
    /// Named sets of environment variables a bacre.yaml can refer to (a restic password, S3 keys, …)
    pub envsets: BTreeMap<String, BTreeMap<String, String>>,
    /// Where the outcome of every job is POSTed
    pub webhook: Option<Webhook>,
    /// What a backend needs from the machine, whatever the service
    pub backends: Backends,
    /// Outside production only: a directory Bacre may fill with made-up services (seed) and
    /// empty again (purge). Nothing outside it is ever written or deleted by either.
    pub sandbox: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Dev,
    #[default]
    Prod,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Dev => "dev",
            Stage::Prod => "prod",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Server {
    pub bind: String,
    pub port: u16,
}

/// From a `username:bcrypt-hash` entry (`htpasswd -nB <username>`)
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub username: String,
    pub password_hash: String,
}

/// With a secret, the body is signed (HMAC-SHA256, `X-Bacre-Signature`)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Webhook {
    pub url: String,
    pub secret: Option<String>,
}

/// A backend without a block here is not set up on this daemon, and a bacre.yaml that asks for
/// it is refused. btrfs needs nothing beyond its tools.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct Backends {
    #[serde(default)]
    pub restic: Option<ResticBackend>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResticBackend {
    pub cache_dir: PathBuf,
    /// Where downloaded snapshots wait to be inspected and restored: <stagingDir>/<service>/<snapshot>
    pub staging_dir: PathBuf,
}

/// The file as written, before it is checked
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    #[serde(default)]
    stage: Stage,
    server: Server,
    #[serde(default)]
    services: Vec<String>,
    tmp_dir: Option<PathBuf>,
    #[serde(default)]
    users: Vec<String>,
    #[serde(default)]
    envsets: BTreeMap<String, BTreeMap<String, String>>,
    webhook: Option<Webhook>,
    #[serde(default)]
    backends: Backends,
    sandbox: Option<PathBuf>,
}

impl Config {
    pub fn parse(yaml: &str) -> Result<Config, String> {
        let raw: Raw = serde_yaml_ng::from_str(yaml).map_err(|e| e.to_string())?;

        if raw.server.bind.is_empty() {
            return Err("server.bind: must not be empty".to_string());
        }
        if raw.server.port == 0 {
            return Err("server.port: must be a port number".to_string());
        }
        if raw.services.iter().any(String::is_empty) {
            return Err("services: a pattern must not be empty".to_string());
        }
        let mut users = Vec::new();
        for entry in &raw.users {
            match entry.split_once(':') {
                Some((username, hash)) if !username.is_empty() && !hash.is_empty() => {
                    users.push(User {
                        username: username.to_string(),
                        password_hash: hash.to_string(),
                    })
                }
                _ => {
                    return Err(format!(
                        "users: invalid user entry (expected username:bcrypt-hash): {entry}"
                    ));
                }
            }
        }
        if raw.envsets.keys().any(String::is_empty)
            || raw
                .envsets
                .values()
                .any(|envset| envset.keys().any(String::is_empty))
        {
            return Err("envsets: names must not be empty".to_string());
        }
        if let Some(webhook) = &raw.webhook {
            if webhook.url.starts_with("https://") {
                return Err("webhook.url: this build can only reach http:// addresses".to_string());
            }
            if !webhook.url.starts_with("http://") {
                return Err("webhook.url: must be an http:// address".to_string());
            }
            if webhook.secret.as_deref() == Some("") {
                return Err("webhook.secret: must not be empty".to_string());
            }
        }
        if let Some(restic) = &raw.backends.restic
            && (restic.cache_dir.as_os_str().is_empty()
                || restic.staging_dir.as_os_str().is_empty())
        {
            return Err("backends.restic: cacheDir and stagingDir must not be empty".to_string());
        }
        match (&raw.sandbox, raw.stage) {
            (Some(_), Stage::Prod) => {
                return Err("sandbox: only outside production (stage: dev)".to_string());
            }
            (Some(sandbox), _) if sandbox.as_os_str().is_empty() => {
                return Err("sandbox: must not be empty".to_string());
            }
            _ => {}
        }

        Ok(Config {
            stage: raw.stage,
            server: raw.server,
            services: raw.services,
            tmp_dir: raw.tmp_dir.unwrap_or_else(std::env::temp_dir),
            users,
            envsets: raw.envsets,
            webhook: raw.webhook,
            backends: raw.backends,
            sandbox: raw.sandbox,
        })
    }

    /// What restic needs from the machine, when this daemon has it set up
    pub fn restic(&self) -> Result<&ResticBackend, String> {
        self.backends.restic.as_ref().ok_or_else(|| {
            "restic is not set up on this daemon: its config has no backends.restic".to_string()
        })
    }

    /// Reads the file and anchors its relative paths to the directory it is in
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let mut config = Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

        let base = std::path::absolute(path)
            .map_err(|e| e.to_string())?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let anchor = |path: &Path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            }
        };

        config.services = config
            .services
            .iter()
            .map(|pattern| anchor(Path::new(pattern)).to_string_lossy().into_owned())
            .collect();
        config.tmp_dir = anchor(&config.tmp_dir);
        if let Some(restic) = &mut config.backends.restic {
            restic.cache_dir = anchor(&restic.cache_dir);
            restic.staging_dir = anchor(&restic.staging_dir);
        }
        // `..` resolved, so the check below sees where the sandbox really is
        config.sandbox = config
            .sandbox
            .as_deref()
            .map(|sandbox| lexical(&anchor(sandbox)));

        // a purge deletes the sandbox whole: it must be a directory of its own, never one the
        // config (and so likely the project) lives in
        if let Some(sandbox) = &config.sandbox
            && (base.starts_with(sandbox) || sandbox.components().count() < 3)
        {
            return Err(format!(
                "{}: sandbox: {} contains the config file or is too close to the root; give it a directory of its own",
                path.display(),
                sandbox.display()
            ));
        }
        Ok(config)
    }
}

/// The path with `.` and `..` worked out on the text alone, without asking the file system
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
pub mod testing {
    use super::Config;

    /// The smallest config there is, with whatever a test adds to it
    pub fn config(extra: &str) -> Config {
        parse(extra).unwrap()
    }

    pub fn parse(extra: &str) -> Result<Config, String> {
        Config::parse(&format!(
            "stage: dev\nserver: {{ bind: 127.0.0.1, port: 3000 }}\nbackends: {{ restic: {{ cacheDir: /cache, stagingDir: /staging }} }}\n{extra}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::testing::{config, parse};
    use super::*;

    #[test]
    fn should_default_what_a_minimal_config_leaves_out() {
        let config = Config::parse("server: { bind: 0.0.0.0, port: 9100 }\nbackends: { restic: { cacheDir: /c, stagingDir: /s } }").unwrap();
        assert_eq!(config.stage, Stage::Prod);
        assert!(config.services.is_empty() && config.users.is_empty() && config.envsets.is_empty());
        assert_eq!(config.webhook, None);
        assert_eq!(config.tmp_dir, std::env::temp_dir());
    }

    #[test]
    fn should_refuse_a_user_entry_without_a_hash() {
        assert!(parse("users: [kim]").is_err());
        assert!(parse("users: ['kim:']").is_err());
        assert_eq!(config("users: ['kim:$2y$05$abc']").users[0].username, "kim");
    }

    #[test]
    fn should_keep_everything_after_the_first_colon_as_the_hash() {
        assert_eq!(config("users: ['kim:a:b']").users[0].password_hash, "a:b");
    }

    #[test]
    fn should_ignore_keys_it_does_not_know() {
        assert!(parse("refreshMinutes: 5").is_ok());
    }

    #[test]
    fn should_anchor_relative_paths_to_the_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(
            &path,
            "server: { bind: 127.0.0.1, port: 3001 }\nservices: ['atlas/*.yaml', /abs/x.yaml]\ntmpDir: .tmp\nbackends: { restic: { cacheDir: .cache, stagingDir: /staging } }\n",
        )
        .unwrap();

        let config = Config::load(&path).unwrap();
        let base = std::path::absolute(dir.path()).unwrap();

        assert_eq!(
            config.services,
            vec![
                base.join("atlas/*.yaml").to_string_lossy().into_owned(),
                "/abs/x.yaml".to_string()
            ]
        );
        assert_eq!(config.tmp_dir, base.join(".tmp"));
        let restic = config.backends.restic.as_ref().unwrap();
        assert_eq!(restic.cache_dir, base.join(".cache"));
        assert_eq!(restic.staging_dir, PathBuf::from("/staging"));
    }

    #[test]
    fn should_run_without_restic_when_the_config_does_not_set_it_up() {
        let config = Config::parse("server: { bind: 127.0.0.1, port: 3001 }").unwrap();
        assert_eq!(config.backends.restic, None);
        assert!(config.restic().unwrap_err().contains("backends.restic"));
        assert!(Config::parse("server: { bind: 127.0.0.1, port: 3001 }\nbackends: { restic: { cacheDir: '', stagingDir: /s } }").is_err());
    }

    #[test]
    fn should_only_allow_a_sandbox_outside_production() {
        assert!(parse("sandbox: .sandbox").is_ok());
        let prod = Config::parse(
            "server: { bind: 0.0.0.0, port: 9100 }\nbackends: { restic: { cacheDir: /c, stagingDir: /s } }\nsandbox: /srv/sandbox",
        );
        assert!(
            prod.unwrap_err()
                .starts_with("sandbox: only outside production")
        );
    }

    #[test]
    fn should_refuse_a_sandbox_that_a_purge_would_take_the_project_down_with() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let load = |sandbox: &str| {
            std::fs::write(&path, format!("stage: dev\nserver: {{ bind: 127.0.0.1, port: 3001 }}\nbackends: {{ restic: {{ cacheDir: .c, stagingDir: .s }} }}\nsandbox: {sandbox}\n")).unwrap();
            Config::load(&path)
        };

        assert_eq!(
            load(".sandbox").unwrap().sandbox,
            Some(std::path::absolute(dir.path()).unwrap().join(".sandbox"))
        );
        for bad in [".", "..", "/", "/srv"] {
            assert!(load(bad).is_err(), "{bad} should be refused");
        }
    }
}
