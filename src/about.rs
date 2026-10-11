//! Which Unseat this is (version, commit, release channel) and whether a newer release
//! exists. Everything here comes from build metadata (`Cargo.toml` and `build.rs`), never
//! from hand-written strings.

use serde::Deserialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Short Git commit SHA of the build, or empty when it wasn't known.
pub const BUILD_SHA: &str = env!("UNSEAT_BUILD_SHA");
/// The build had uncommitted changes.
pub const BUILD_DIRTY: bool = matches!(env!("UNSEAT_BUILD_DIRTY").as_bytes(), b"1");
const BUILD_CHANNEL: &str = env!("UNSEAT_BUILD_CHANNEL");
/// `repository` from Cargo.toml: the official release source.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Built from a clean `main`.
    Stable,
    /// Built from another branch (a PR or feature build).
    Beta,
    /// Uncommitted changes, or no Git information.
    Dev,
    /// Running as an isolated test copy (UNSEAT_TEST_HOOKS).
    Test,
}

impl Channel {
    pub fn from_build(name: &str) -> Self {
        match name {
            "stable" => Self::Stable,
            "beta" => Self::Beta,
            _ => Self::Dev,
        }
    }

    /// This build's channel; a test copy always says so.
    pub fn current(test_copy: bool) -> Self {
        if test_copy {
            Self::Test
        } else {
            Self::from_build(BUILD_CHANNEL)
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Dev => "Dev",
            Self::Test => "Test",
        }
    }
}

/// "Unseat v0.2.0 · Stable · 5f24733" (SHA omitted when unknown; "+changes" when dirty).
pub fn version_line(channel: Channel, sha: &str, dirty: bool) -> String {
    let mut line = format!("Unseat v{VERSION} · {}", channel.label());
    if !sha.is_empty() {
        line.push_str(&format!(" · {sha}"));
        if dirty {
            line.push_str("+changes");
        }
    }
    line
}

/// `major.minor.patch` from "v1.2.3" or "1.2.3" (a pre-release suffix is ignored).
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let core = tag.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// "owner/name" of a GitHub repository URL.
pub fn github_slug(repository: &str) -> Option<String> {
    let rest = repository.trim_end_matches('/').trim_end_matches(".git").strip_prefix("https://github.com/")?;
    let mut it = rest.split('/');
    let (owner, name) = (it.next()?, it.next()?);
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (it.next().is_none() && ok(owner) && ok(name)).then(|| format!("{owner}/{name}"))
}

/// The GitHub API address for the latest published release.
pub fn latest_release_api(repository: &str) -> Option<String> {
    github_slug(repository).map(|slug| format!("https://api.github.com/repos/{slug}/releases/latest"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// Release page; only ever a page of this repository on github.com.
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Parse GitHub's "latest release" response. The page link must belong to `repository`, so
/// a tampered response can't send the user elsewhere.
pub fn parse_latest_release(body: &[u8], repository: &str) -> Option<Release> {
    let r: ApiRelease = serde_json::from_slice(body).ok()?;
    let slug = github_slug(repository)?;
    let page = format!("https://github.com/{slug}/releases/");
    (!r.draft && !r.prerelease && parse_version(&r.tag_name).is_some() && r.html_url.starts_with(&page) && !r.html_url.contains(['"', ' ', '\n']))
        .then_some(Release { tag: r.tag_name, url: r.html_url })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    UpToDate { latest: String },
    Available(Release),
    Failed(String),
}

/// Compare the published release with this version.
pub fn judge(latest: Option<Release>, current: &str) -> UpdateCheck {
    match latest {
        Some(r) if is_newer(&r.tag, current) => UpdateCheck::Available(r),
        Some(r) => UpdateCheck::UpToDate { latest: r.tag },
        None => UpdateCheck::Failed("unexpected reply from GitHub".into()),
    }
}
