pub const VERSION: &str = env!("BACRE_VERSION");

pub const COMMIT: &str = env!("BACRE_COMMIT");

pub fn commit() -> &'static str {
    &COMMIT[..COMMIT.len().min(7)]
}
