//! WebSocket connections, optionally through an HTTP (CONNECT) or SOCKS5 proxy.

use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

pub type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("bad proxy setting: {0}")]
    BadProxy(String),
    #[error("proxy: {0}")]
    Proxy(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("timed out")]
    Timeout,
}

/// Resolves the `proxy` setting: "" uses the proxy from the environment
/// (HTTPS_PROXY, ALL_PROXY) or, on Windows and macOS, the one in the system
/// settings, where VPN apps set it; "none" disables the proxy; anything else
/// must be an http:// or socks5:// URL.
pub fn resolve_proxy(setting: &str) -> Result<Option<Url>, NetError> {
    let raw = match setting.trim() {
        "none" => return Ok(None),
        "" => {
            let from_env = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
            match from_env.or_else(system_proxy) {
                Some(v) => v,
                None => return Ok(None),
            }
        }
        s => s.to_owned(),
    };
    let url = Url::parse(&raw).map_err(|e| NetError::BadProxy(e.to_string()))?;
    match url.scheme() {
        "http" | "socks5" | "socks5h" if url.host_str().is_some() => Ok(Some(url)),
        "http" | "socks5" | "socks5h" => Err(NetError::BadProxy("the proxy URL has no host".into())),
        s => Err(NetError::BadProxy(format!("unsupported proxy scheme {s:?} (use http or socks5)"))),
    }
}

/// A proxy for showing to the user: scheme, host and port, without the
/// user name or password.
pub fn proxy_label(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), url.host_str().unwrap_or_default()),
        None => format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default()),
    }
}

/// The proxy in Windows' settings (Settings > Network > Proxy), if it's on.
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let (enable, server) = (wide("ProxyEnable"), wide("ProxyServer"));
    let mut enabled = 0u32;
    let mut size = 4u32;
    // SAFETY: reads a DWORD into a 4-byte buffer; the strings are NUL-terminated.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            enable.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut enabled).cast(),
            &mut size,
        )
    };
    if r != 0 || enabled == 0 {
        return None;
    }
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: reads a string into `buf`, whose size in bytes is passed along.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            server.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if r != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    windows_proxy_url(&String::from_utf16_lossy(&buf[..len]))
}

/// The proxy in macOS' network settings, if one is on.
#[cfg(target_os = "macos")]
fn system_proxy() -> Option<String> {
    let out = std::process::Command::new("scutil").arg("--proxy").output().ok()?;
    scutil_proxy_url(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_proxy() -> Option<String> {
    None
}

/// Windows' proxy setting as a URL. It is either "host:port" for everything,
/// or per protocol, like "http=host:port;https=host:port;socks=host:port".
#[cfg(any(windows, test))]
fn windows_proxy_url(server: &str) -> Option<String> {
    let (mut all, mut http, mut https, mut socks) = (None, None, None, None);
    for part in server.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("https") => https = Some(v.trim()),
            Some((k, v)) if k.eq_ignore_ascii_case("http") => http = Some(v.trim()),
            Some((k, v)) if k.eq_ignore_ascii_case("socks") => socks = Some(v.trim()),
            Some(_) => {}
            None => all = Some(part),
        }
    }
    let url =
        |scheme: &str, addr: &str| if addr.contains("://") { addr.to_owned() } else { format!("{scheme}://{addr}") };
    match https.or(all).or(http) {
        Some(a) => Some(url("http", a)),
        None => socks.map(|a| url("socks5", a)),
    }
}

/// The proxy from `scutil --proxy` output: HTTPS first, then SOCKS, then HTTP.
#[cfg(any(target_os = "macos", test))]
fn scutil_proxy_url(out: &str) -> Option<String> {
    let get = |key: &str| {
        out.lines().find_map(|l| {
            let (k, v) = l.split_once(" : ")?;
            (k.trim() == key).then(|| v.trim().to_owned())
        })
    };
    for (prefix, scheme) in [("HTTPS", "http"), ("SOCKS", "socks5"), ("HTTP", "http")] {
        if get(&format!("{prefix}Enable")).as_deref() == Some("1")
            && let (Some(host), Some(port)) = (get(&format!("{prefix}Proxy")), get(&format!("{prefix}Port")))
        {
            return Some(format!("{scheme}://{host}:{port}"));
        }
    }
    None
}

/// Selects the TLS crypto implementation once per process.
pub fn init_tls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Opens a WebSocket to `url` with extra request headers.
pub async fn connect_ws(url: &str, headers: &[(&str, &str)], proxy: Option<&Url>) -> Result<Ws, NetError> {
    init_tls();
    let mut req = url.into_client_request()?;
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(std::io::Error::other)?;
        let value = HeaderValue::from_str(v).map_err(std::io::Error::other)?;
        req.headers_mut().insert(name, value);
    }
    let host = req.uri().host().unwrap_or_default().to_owned();
    let port = req.uri().port_u16().unwrap_or(if req.uri().scheme_str() == Some("ws") { 80 } else { 443 });
    let fut = async {
        let tcp = tcp_to(&host, port, proxy).await?;
        tcp.set_nodelay(true)?;
        let (ws, _) = tokio_tungstenite::client_async_tls_with_config(req, tcp, None, None).await?;
        Ok(ws)
    };
    tokio::time::timeout(CONNECT_TIMEOUT, fut).await.map_err(|_| NetError::Timeout)?
}

/// A TCP connection to host:port, tunnelled through the proxy if there is one.
pub async fn tcp_to(host: &str, port: u16, proxy: Option<&Url>) -> Result<TcpStream, NetError> {
    let Some(p) = proxy else {
        return Ok(TcpStream::connect((host, port)).await?);
    };
    let phost = p.host_str().unwrap_or_default();
    let user = percent_decode(p.username());
    let pass = p.password().map(percent_decode);
    match p.scheme() {
        "http" => {
            let mut s = TcpStream::connect((phost, p.port().unwrap_or(8080))).await?;
            let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
            if !user.is_empty() {
                let token = base64::engine::general_purpose::STANDARD
                    .encode(format!("{user}:{}", pass.as_deref().unwrap_or_default()));
                req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
            }
            req.push_str("\r\n");
            s.write_all(req.as_bytes()).await?;
            let head = read_head(&mut s).await?;
            let status = head.split_whitespace().nth(1).unwrap_or("");
            if status != "200" {
                let line = head.lines().next().unwrap_or("").trim();
                return Err(NetError::Proxy(format!("CONNECT refused: {line}")));
            }
            Ok(s)
        }
        _ => {
            let addr = (phost, p.port().unwrap_or(1080));
            let s = if user.is_empty() {
                tokio_socks::tcp::Socks5Stream::connect(addr, (host, port)).await
            } else {
                tokio_socks::tcp::Socks5Stream::connect_with_password(
                    addr,
                    (host, port),
                    &user,
                    pass.as_deref().unwrap_or_default(),
                )
                .await
            };
            Ok(s.map_err(|e| NetError::Proxy(e.to_string()))?.into_inner())
        }
    }
}

/// Reads an HTTP response head, up to the blank line.
async fn read_head(s: &mut TcpStream) -> Result<String, NetError> {
    let mut buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if s.read(&mut byte).await? == 0 {
            return Err(NetError::Proxy("the proxy closed the connection".into()));
        }
        buf.push(byte[0]);
        if buf.len() > 16 * 1024 {
            return Err(NetError::Proxy("response too long".into()));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_setting() {
        assert!(resolve_proxy("none").unwrap().is_none());
        let p = resolve_proxy("http://u%40x:p%3Aw@proxy.local:3128").unwrap().unwrap();
        assert_eq!(p.port(), Some(3128));
        assert_eq!(percent_decode(p.username()), "u@x");
        assert_eq!(percent_decode(p.password().unwrap()), "p:w");
        assert!(resolve_proxy("ftp://proxy").is_err());
        assert!(resolve_proxy("socks5://127.0.0.1:1080").unwrap().is_some());
        assert_eq!(proxy_label(&p), "http://proxy.local:3128");
    }

    #[test]
    fn windows_proxy_setting() {
        assert_eq!(windows_proxy_url("127.0.0.1:10808").as_deref(), Some("http://127.0.0.1:10808"));
        assert_eq!(
            windows_proxy_url("http=127.0.0.1:10809;https=127.0.0.1:10810;socks=127.0.0.1:10808").as_deref(),
            Some("http://127.0.0.1:10810")
        );
        assert_eq!(windows_proxy_url("socks=127.0.0.1:1080").as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(windows_proxy_url("ftp=1.2.3.4:21"), None);
        assert_eq!(windows_proxy_url(""), None);
    }

    #[test]
    fn macos_proxy_setting() {
        let out = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  HTTPEnable : 1\n  HTTPPort : 10809\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 0\n  SOCKSEnable : 1\n  SOCKSPort : 10808\n  SOCKSProxy : 127.0.0.1\n}\n";
        assert_eq!(scutil_proxy_url(out).as_deref(), Some("socks5://127.0.0.1:10808"));
        assert_eq!(scutil_proxy_url("<dictionary> {\n  HTTPEnable : 0\n}"), None);
    }
}
