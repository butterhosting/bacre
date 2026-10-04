use std::process::Command;

fn main() {
    // the folder has to exist to compile at all; `just build` fills it, `cargo run` leaves it empty
    std::fs::create_dir_all("website/dist").expect("could not create website/dist");
    println!("cargo:rerun-if-changed=website/dist");

    // the tag the commit carries, else the closest tag marked as a snapshot, else 0.0.0
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

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}
