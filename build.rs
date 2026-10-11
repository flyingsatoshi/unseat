use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/unseat.ico");
    println!("cargo:rerun-if-changed=unseat.rc");
    println!("cargo:rerun-if-changed=unseat.manifest");
    build_info();
    if std::env::var_os("CARGO_FEATURE_GUI").is_none() {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap();
    let macros = [
        format!("UNSEAT_VERSION=\"{version}\""),
        format!("UNSEAT_VERSION_NUM={major},{minor},{patch},0"),
    ];
    let resources = embed_resource::compile_for("unseat.rc", ["unseat"], &macros);
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        resources
            .manifest_required()
            .expect("embed Unseat identity and application manifest");
    } else {
        resources
            .manifest_optional()
            .expect("compile optional development resources");
    }
}

/// Which commit this is, and from which branch, for the version line in Settings. Builds
/// without `.git` (the Docker cross-build copies the tree without it) get the values from
/// `UNSEAT_GIT_SHA`, `UNSEAT_GIT_BRANCH` and `UNSEAT_GIT_DIRTY`; `UNSEAT_CHANNEL` overrides.
fn build_info() {
    for var in ["UNSEAT_GIT_SHA", "UNSEAT_GIT_BRANCH", "UNSEAT_GIT_DIRTY", "UNSEAT_CHANNEL"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    for path in [".git/HEAD", ".git/index"] {
        if std::path::Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let sha = std::env::var("UNSEAT_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| git(&["rev-parse", "--short=7", "HEAD"]))
        .unwrap_or_default();
    let branch = std::env::var("UNSEAT_GIT_BRANCH")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("GITHUB_HEAD_REF").ok().filter(|s| !s.is_empty()))
        .or_else(|| git(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .unwrap_or_default();
    let dirty = match std::env::var("UNSEAT_GIT_DIRTY") {
        Ok(v) => v == "1",
        Err(_) => git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty()),
    };
    let sha: String = sha.chars().filter(|c| c.is_ascii_hexdigit()).take(12).collect();
    let channel = std::env::var("UNSEAT_CHANNEL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        if sha.is_empty() || dirty {
            "dev".into()
        } else if branch == "main" {
            "stable".into()
        } else {
            "beta".into()
        }
    });
    println!("cargo:rustc-env=UNSEAT_BUILD_SHA={sha}");
    println!("cargo:rustc-env=UNSEAT_BUILD_DIRTY={}", u8::from(dirty));
    println!("cargo:rustc-env=UNSEAT_BUILD_CHANNEL={channel}");
}
