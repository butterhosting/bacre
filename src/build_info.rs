//! What this binary was built from; `build.rs` reads both off git (or the environment).

/// The tag the build is on, `<closest tag>-snapshot` past one, `0.0.0` without any
pub const VERSION: &str = env!("BACRE_VERSION");

/// The full commit hash, all zeros outside a git checkout
pub const COMMIT: &str = env!("BACRE_COMMIT");

/// The short form, as shown to people
pub fn commit() -> &'static str {
    &COMMIT[..COMMIT.len().min(7)]
}
