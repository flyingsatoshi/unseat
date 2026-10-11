//! "Check for updates" in Settings: one HTTPS GET to GitHub's latest-release API for the
//! repository in Cargo.toml, only when the user asks. It reports; it never downloads or
//! installs anything. The answer is shown in the Settings window.

use crate::win::{settings_dlg, testhook, winhttp};
use std::sync::Mutex;
use unseat::about::{self, UpdateCheck};

const TIMEOUT_MS: i32 = 10_000;
/// GitHub's release JSON is a few KB; anything far larger isn't a release reply.
const MAX_REPLY: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    Done(UpdateCheck),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or(State::Idle)
}

/// The address to ask. Test copies may point it at a local fake (UNSEAT_TEST_UPDATE_URL).
fn api_url() -> Option<String> {
    if testhook::enabled() {
        if let Ok(url) = std::env::var("UNSEAT_TEST_UPDATE_URL") {
            return Some(url);
        }
    }
    about::latest_release_api(about::REPOSITORY).filter(|u| u.starts_with("https://api.github.com/"))
}

/// Start a check in the background unless one is running.
pub fn start() {
    {
        let Ok(mut s) = STATE.lock() else { return };
        if *s == State::Checking {
            return;
        }
        *s = State::Checking;
    }
    settings_dlg::repaint();
    let _ = std::thread::Builder::new().name("unseat-update-check".into()).spawn(|| {
        let result = match api_url() {
            None => UpdateCheck::Failed("no release source".into()),
            Some(url) => {
                let headers = [
                    ("User-Agent", format!("Unseat/{}", about::VERSION)),
                    ("Accept", "application/vnd.github+json".to_string()),
                ];
                match winhttp::get(&url, &headers, TIMEOUT_MS, MAX_REPLY) {
                    Ok((200, body)) => about::judge(about::parse_latest_release(&body, about::REPOSITORY), about::VERSION),
                    Ok((404, _)) => UpdateCheck::Failed("no published release".into()),
                    Ok((code, _)) => UpdateCheck::Failed(format!("GitHub answered {code}")),
                    Err(_) => UpdateCheck::Failed("no connection".into()),
                }
            }
        };
        testhook::log(&format!("update check {result:?}"));
        if let Ok(mut s) = STATE.lock() {
            *s = State::Done(result);
        }
        settings_dlg::repaint();
    });
}
