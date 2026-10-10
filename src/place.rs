use std::path::Path;
use std::time::SystemTime;

pub fn is_dev_build(exe: &Path) -> bool {
    let parts: Vec<String> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let Some(i) = parts.iter().position(|p| p == "target") else {
        return false;
    };
    match parts.get(i + 1).map(|s| s.as_str()) {
        Some("debug" | "release") => true,
        Some(next) => {
            next.contains("windows")
                || next.contains("msvc")
                || next.contains("gnu")
                || next.contains("linux")
                || next.contains("darwin")
                || next.contains("apple")
        }
        None => false,
    }
}

/// Whether a new launch should replace the Unseat already running from `running`.
///
/// Only a different, newer executable takes over, so opening an updated build always runs
/// the new code while reopening the same exe (or an older copy) just shows the running one.
/// A running exe whose file has gone away was replaced or deleted, so it counts as older.
pub fn newer_build_takes_over(
    running: &Path,
    running_built: Option<SystemTime>,
    mine: &Path,
    mine_built: Option<SystemTime>,
) -> bool {
    let same = running
        .to_string_lossy()
        .eq_ignore_ascii_case(&mine.to_string_lossy());
    match (same, running_built, mine_built) {
        (true, _, _) | (_, _, None) => false,
        (false, None, Some(_)) => true,
        (false, Some(theirs), Some(ours)) => ours > theirs,
    }
}
