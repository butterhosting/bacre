//! Two things the server needs at compile time: a `website/dist` folder to embed, and the
//! version and commit it is built from.

use std::process::Command;

fn main() {
    // The website is embedded from `website/dist`, which `just build-website` fills. The
    // folder has to exist for the server to compile at all, so a checkout that has not built
    // the website yet (every `cargo run` during development) gets an empty one.
    std::fs::create_dir_all("website/dist").expect("could not create website/dist");
    // a rebuilt website is embedded into the next build of the server
    println!("cargo:rerun-if-changed=website/dist");

    // A pipeline can state both; otherwise git is asked, the way a release would be cut:
    // the tag the commit carries, else the closest tag marked as a snapshot, else 0.0.0.
    println!("cargo:rerun-if-env-changed=BACRE_VERSION");
    println!("cargo:rerun-if-env-changed=BACRE_COMMIT");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let commit = std::env::var("BACRE_COMMIT")
        .ok()
        .filter(|commit| !commit.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "0".repeat(40));
    let version = std::env::var("BACRE_VERSION")
        .ok()
        .filter(|version| !version.is_empty())
        .or_else(|| {
            git(&["tag", "--points-at", "HEAD"])
                .and_then(|tags| tags.lines().next().map(str::to_string))
        })
        .or_else(|| git(&["describe", "--tags", "--abbrev=0"]).map(|tag| format!("{tag}-snapshot")))
        .unwrap_or_else(|| "0.0.0".to_string());

    println!("cargo:rustc-env=BACRE_COMMIT={commit}");
    println!("cargo:rustc-env=BACRE_VERSION={version}");
}

/// The trimmed output of a git command, or nothing when it fails or prints nothing
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}
