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
    #[error("timed out connecting")]
    Timeout,
}

/// Resolves the `proxy` setting: "" falls back to the environment, "none"
/// disables the proxy, anything else must be an http:// or socks5:// URL.
pub fn resolve_proxy(setting: &str) -> Result<Option<Url>, NetError> {
    let raw = match setting.trim() {
        "none" => return Ok(None),
        "" => {
            let from_env = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
            match from_env {
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
    }
}
