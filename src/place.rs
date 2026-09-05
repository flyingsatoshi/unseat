use std::path::{Path, PathBuf};

pub fn installed_exe(localappdata: &Path) -> PathBuf {
    localappdata.join("Unseat").join("unseat.exe")
}

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
