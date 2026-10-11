use unseat::about::{
    github_slug, is_newer, judge, latest_release_api, parse_latest_release, parse_version, version_line, Channel,
    Release, UpdateCheck, REPOSITORY, VERSION,
};

const REPO: &str = "https://github.com/flyingsatoshi/unseat";

#[test]
fn version_comes_from_cargo_and_the_line_names_channel_and_commit() {
    assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    assert_eq!(version_line(Channel::Stable, "5f24733", false), format!("Unseat v{VERSION} · Stable · 5f24733"));
    assert_eq!(version_line(Channel::Dev, "5f24733", true), format!("Unseat v{VERSION} · Dev · 5f24733+changes"));
    assert_eq!(version_line(Channel::Beta, "", false), format!("Unseat v{VERSION} · Beta"));
}

#[test]
fn channels_from_the_build_and_test_copies() {
    assert_eq!(Channel::from_build("stable"), Channel::Stable);
    assert_eq!(Channel::from_build("beta"), Channel::Beta);
    assert_eq!(Channel::from_build("dev"), Channel::Dev);
    assert_eq!(Channel::from_build("anything"), Channel::Dev);
    assert_eq!(Channel::current(true), Channel::Test, "a test copy always says Test");
}

#[test]
fn versions_compare_numerically() {
    assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
    assert_eq!(parse_version("1.10.3-beta.1"), Some((1, 10, 3)));
    assert_eq!(parse_version("v1.2"), None);
    assert_eq!(parse_version("v1.2.3.4"), None);
    assert_eq!(parse_version("latest"), None);
    assert!(is_newer("v0.10.0", "0.9.9"));
    assert!(is_newer("v0.2.1", "0.2.0"));
    assert!(!is_newer("v0.2.0", "0.2.0"));
    assert!(!is_newer("v0.1.1", "0.2.0"));
    assert!(!is_newer("garbage", "0.2.0"));
}

#[test]
fn the_release_source_is_the_cargo_repository_on_github() {
    assert_eq!(REPOSITORY, REPO, "Cargo.toml repository is the official source");
    assert_eq!(github_slug(REPO).as_deref(), Some("flyingsatoshi/unseat"));
    assert_eq!(github_slug("https://github.com/a/b.git").as_deref(), Some("a/b"));
    assert_eq!(github_slug("https://example.com/a/b"), None);
    assert_eq!(github_slug("https://github.com/a/b/c"), None);
    assert_eq!(github_slug("https://github.com/a/b?x=1"), None);
    assert_eq!(latest_release_api(REPO).as_deref(), Some("https://api.github.com/repos/flyingsatoshi/unseat/releases/latest"));
}

#[test]
fn release_replies_are_parsed_strictly() {
    let ok = br#"{"tag_name":"v0.3.0","html_url":"https://github.com/flyingsatoshi/unseat/releases/tag/v0.3.0","draft":false,"prerelease":false,"assets":[]}"#;
    let r = parse_latest_release(ok, REPO).unwrap();
    assert_eq!(r, Release { tag: "v0.3.0".into(), url: "https://github.com/flyingsatoshi/unseat/releases/tag/v0.3.0".into() });
    let elsewhere = br#"{"tag_name":"v9.0.0","html_url":"https://evil.example/releases/v9"}"#;
    assert_eq!(parse_latest_release(elsewhere, REPO), None, "page must be on this repository");
    let other_repo = br#"{"tag_name":"v9.0.0","html_url":"https://github.com/someone/unseat/releases/tag/v9.0.0"}"#;
    assert_eq!(parse_latest_release(other_repo, REPO), None);
    let pre = br#"{"tag_name":"v9.0.0","html_url":"https://github.com/flyingsatoshi/unseat/releases/tag/v9.0.0","prerelease":true}"#;
    assert_eq!(parse_latest_release(pre, REPO), None);
    assert_eq!(parse_latest_release(b"not json", REPO), None);
    assert_eq!(parse_latest_release(br#"{"message":"Not Found"}"#, REPO), None);
}

#[test]
fn update_verdicts() {
    let rel = |t: &str| Some(Release { tag: t.into(), url: format!("{REPO}/releases/tag/{t}") });
    assert!(matches!(judge(rel("v0.3.0"), "0.2.0"), UpdateCheck::Available(_)));
    assert_eq!(judge(rel("v0.1.1"), "0.2.0"), UpdateCheck::UpToDate { latest: "v0.1.1".into() });
    assert_eq!(judge(rel("v0.2.0"), "0.2.0"), UpdateCheck::UpToDate { latest: "v0.2.0".into() });
    assert!(matches!(judge(None, "0.2.0"), UpdateCheck::Failed(_)));
}
