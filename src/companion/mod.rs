//! iPhone reminders. Pure logic, unit-tested on any OS: pairing, the device store, the API,
//! the reminder policy, the delivery queue and Web Push encryption. The Windows host
//! (`src/win/phone.rs`) owns sockets, WinHTTP, DPAPI and threads.

pub mod api;
pub mod http;
pub mod outbox;
pub mod pairing;
pub mod reminder;
pub mod store;
pub mod webpush;

/// Contact URL sent in the VAPID token (Apple requires an `https:` or `mailto:` subject).
pub const VAPID_SUBJECT: &str = "https://github.com/flyingsatoshi/unseat";
pub const PHONE_PORT: u16 = 8790;
pub const ADMIN_PORT: u16 = 8791;

/// A QR code as a standalone SVG.
pub fn qr_svg(text: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(qr) = QrCode::encode_text(text, QrCodeEcc::Medium) else {
        return String::new();
    };
    let border = 4;
    let size = qr.size() + border * 2;
    let mut path = String::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges" role="img" aria-label="Pairing QR code"><rect width="100%" height="100%" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##
    )
}

/// Files served to the phone and to the PC page, embedded in the executable.
pub mod assets {
    pub type Asset = (&'static str, &'static [u8]);

    pub fn phone(path: &str) -> Option<Asset> {
        Some(match path {
            "/" | "/index.html" => ("text/html; charset=utf-8", include_bytes!("web/index.html")),
            "/app.js" => ("text/javascript; charset=utf-8", include_bytes!("web/app.js")),
            "/sw.js" => ("text/javascript; charset=utf-8", include_bytes!("web/sw.js")),
            "/manifest.webmanifest" => ("application/manifest+json", include_bytes!("web/manifest.webmanifest")),
            "/icon-180.png" => ("image/png", include_bytes!("web/icon-180.png")),
            "/icon-512.png" => ("image/png", include_bytes!("web/icon-512.png")),
            _ => return None,
        })
    }

    pub fn admin(path: &str) -> Option<Asset> {
        Some(match path {
            "/" | "/index.html" => ("text/html; charset=utf-8", include_bytes!("web/admin.html")),
            "/admin.js" => ("text/javascript; charset=utf-8", include_bytes!("web/admin.js")),
            "/icon-180.png" => ("image/png", include_bytes!("web/icon-180.png")),
            _ => return None,
        })
    }
}
