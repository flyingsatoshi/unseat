//! A small, strict HTTP/1.1 request parser and response writer for the loopback listeners.
//! One request per connection; no chunked bodies, no keep-alive, tight size limits.

pub const MAX_HEAD: usize = 8 * 1024;
pub const MAX_BODY: usize = 16 * 1024;
const MAX_HEADERS: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    /// Lower-cased names; a repeated header keeps its first value.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k == name).then(|| percent_decode(v))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// More bytes are needed.
    Incomplete,
    /// 400
    Bad,
    /// 413
    TooLarge,
    /// 501: chunked or other transfer codings.
    Unsupported,
}

impl ParseError {
    pub fn status(self) -> u16 {
        match self {
            Self::Incomplete | Self::Bad => 400,
            Self::TooLarge => 413,
            Self::Unsupported => 501,
        }
    }
}

/// Parse a complete request from `buf`. Returns `Incomplete` until the head and the whole
/// declared body have arrived.
pub fn parse(buf: &[u8]) -> Result<Request, ParseError> {
    let Some(end) = find(buf, b"\r\n\r\n") else {
        return Err(if buf.len() > MAX_HEAD { ParseError::TooLarge } else { ParseError::Incomplete });
    };
    if end > MAX_HEAD {
        return Err(ParseError::TooLarge);
    }
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| ParseError::Bad)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().ok_or(ParseError::Bad)?.split(' ');
    let (Some(method), Some(target), Some(version), None) = (first.next(), first.next(), first.next(), first.next()) else {
        return Err(ParseError::Bad);
    };
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ParseError::Bad);
    }
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(ParseError::Bad);
    }
    if !target.starts_with('/') || target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(ParseError::Bad);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path.contains("..") || path.contains('\\') || path.contains("//") {
        return Err(ParseError::Bad);
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if headers.len() >= MAX_HEADERS {
            return Err(ParseError::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(ParseError::Bad)?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(ParseError::Bad);
        }
        let name = name.to_ascii_lowercase();
        if headers.iter().any(|(k, _)| *k == name) {
            // Two Host or Content-Length headers are an attack, not a mistake.
            if name == "host" || name == "content-length" || name == "authorization" || name.starts_with("tailscale-") {
                return Err(ParseError::Bad);
            }
            continue;
        }
        headers.push((name, value.trim().to_string()));
    }
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    if get("transfer-encoding").is_some() {
        return Err(ParseError::Unsupported);
    }
    if get("host").is_none() {
        return Err(ParseError::Bad);
    }
    let len = match get("content-length") {
        None => 0,
        Some(v) if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) && v.len() <= 9 => v.parse::<usize>().map_err(|_| ParseError::Bad)?,
        Some(_) => return Err(ParseError::Bad),
    };
    if len > MAX_BODY {
        return Err(ParseError::TooLarge);
    }
    let body_start = end + 4;
    if buf.len() < body_start + len {
        return Err(ParseError::Incomplete);
    }
    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        headers,
        body: buf[body_start..body_start + len].to_vec(),
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// Allow the browser to cache (only the app shell, revalidated each time).
    pub cacheable: bool,
}

impl Response {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self { status, content_type: "application/json; charset=utf-8", body: value.to_string().into_bytes(), cacheable: false }
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": message }))
    }

    pub fn asset(content_type: &'static str, body: &[u8]) -> Self {
        Self { status: 200, content_type, body: body.to_vec(), cacheable: true }
    }

    /// The full response, with security headers on everything.
    pub fn to_bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            421 => "Misdirected Request",
            429 => "Too Many Requests",
            501 => "Not Implemented",
            503 => "Service Unavailable",
            _ => "Error",
        };
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: {}\r\n{}\r\n",
            self.status,
            self.content_type,
            self.body.len(),
            if self.cacheable { "no-cache" } else { "no-store" },
            SECURITY_HEADERS
        )
        .into_bytes();
        head.extend_from_slice(&self.body);
        head
    }
}

pub const SECURITY_HEADERS: &str = concat!(
    "X-Content-Type-Options: nosniff\r\n",
    "Referrer-Policy: no-referrer\r\n",
    "X-Frame-Options: DENY\r\n",
    "Cross-Origin-Opener-Policy: same-origin\r\n",
    "Cross-Origin-Resource-Policy: same-origin\r\n",
    "Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; ",
    "connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\n",
);
