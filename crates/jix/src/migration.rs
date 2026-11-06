//! Release-matched, self-contained migration instructions. No project operations.
pub const GUIDE: &str = include_str!("../docs/migration.md");

pub fn guide() -> String {
    format!(
        "# Migrating to Jix {}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        GUIDE
    )
}
