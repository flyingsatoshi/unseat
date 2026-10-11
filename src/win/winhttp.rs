//! One HTTP request through WinHTTP: Windows' own TLS, proxy settings and certificate store,
//! so Unseat carries no TLS stack of its own.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn open(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what}: {}", windows::core::Error::from_win32().message()))
    } else {
        Ok(Handle(h))
    }
}

/// POST `body` to `url` (https, or http for test endpoints). Returns the HTTP status, or an
/// error string for network failures (which the queue retries).
pub fn post(url: &str, headers: &[(&str, String)], body: &[u8], timeout_ms: i32) -> Result<u16, String> {
    request("POST", url, headers, body, timeout_ms, 0).map(|(status, _)| status)
}

/// GET `url` and read at most `max_body` bytes of the reply (more is an error).
pub fn get(url: &str, headers: &[(&str, String)], timeout_ms: i32, max_body: usize) -> Result<(u16, Vec<u8>), String> {
    request("GET", url, headers, &[], timeout_ms, max_body)
}

fn request(method: &str, url: &str, headers: &[(&str, String)], body: &[u8], timeout_ms: i32, max_body: usize) -> Result<(u16, Vec<u8>), String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("unsupported URL".into());
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "bad port".to_string())?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    let mut header_text = String::new();
    for (k, v) in headers {
        if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
            return Err("bad header".into());
        }
        header_text.push_str(&format!("{k}: {v}\r\n"));
    }
    let header_wide: Vec<u16> = header_text.encode_utf16().collect();
    unsafe {
        let session = open(
            WinHttpOpen(&HSTRING::from("Unseat"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "open",
        )?;
        // Resolve, connect and send get half the budget each and the response the whole of it,
        // so one unreachable push service holds the sender for well under a reminder's life.
        let _ = WinHttpSetTimeouts(session.0, timeout_ms / 2, timeout_ms / 2, timeout_ms / 2, timeout_ms);
        let connect = open(WinHttpConnect(session.0, &HSTRING::from(host), port, 0), "connect")?;
        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = open(
            WinHttpOpenRequest(connect.0, &HSTRING::from(method), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags),
            "request",
        )?;
        let len = body.len() as u32;
        let body_ptr = if body.is_empty() { None } else { Some(body.as_ptr().cast()) };
        let header_arg = if header_wide.is_empty() { None } else { Some(header_wide.as_slice()) };
        WinHttpSendRequest(request.0, header_arg, body_ptr, len, len, 0).map_err(|e| format!("send: {}", e.message()))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("receive: {}", e.message()))?;
        let mut status: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("status: {}", e.message()))?;
        let mut reply = Vec::new();
        if max_body > 0 {
            let mut chunk = [0u8; 8192];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read)
                    .map_err(|e| format!("read: {}", e.message()))?;
                if read == 0 {
                    break;
                }
                reply.extend_from_slice(&chunk[..read as usize]);
                if reply.len() > max_body {
                    return Err("reply too large".into());
                }
            }
        }
        Ok((status as u16, reply))
    }
}
