//! The daemon's local socket (a Unix socket, or a named pipe on Windows): one
//! JSON request per connection, one JSON reply.

use std::io;
use std::time::Duration;

use mirza_core::ipc::{Reply, Request, socket_path};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::daemon::Msg;

pub use mirza_core::ipc::request;

/// Answers one connection.
async fn handle<S: AsyncRead + AsyncWrite + Unpin>(stream: S, tx: mpsc::UnboundedSender<Msg>) {
    let (r, mut w) = tokio::io::split(stream);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(5), BufReader::new(r).read_line(&mut line)).await;
    if !matches!(read, Ok(Ok(n)) if n > 0) {
        return;
    }
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) => {
            let (rtx, rrx) = oneshot::channel();
            if tx.send(Msg::Ipc(req, rtx)).is_err() {
                return;
            }
            rrx.await.unwrap_or_else(|_| Reply::err("the daemon is shutting down"))
        }
        Err(e) => Reply::err(format!("bad request: {e}")),
    };
    let mut out = serde_json::to_string(&reply).unwrap_or_default();
    out.push('\n');
    let _ = w.write_all(out.as_bytes()).await;
}

#[cfg(unix)]
pub struct Listener(tokio::net::UnixListener);

/// Binds the socket. Fails with AddrInUse if another daemon is running.
#[cfg(unix)]
pub async fn listen() -> io::Result<Listener> {
    use std::os::unix::fs::PermissionsExt;
    let path = socket_path();
    if tokio::net::UnixStream::connect(&path).await.is_ok() {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "Mirza is already running"));
    }
    let _ = std::fs::remove_file(&path); // left over from a crash
    let listener = tokio::net::UnixListener::bind(&path)?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok(Listener(listener))
}

#[cfg(unix)]
pub async fn serve(listener: Listener, tx: mpsc::UnboundedSender<Msg>) {
    loop {
        let Ok((stream, _)) = listener.0.accept().await else { continue };
        tokio::spawn(handle(stream, tx.clone()));
    }
}

#[cfg(windows)]
pub struct Listener(tokio::net::windows::named_pipe::NamedPipeServer);

#[cfg(windows)]
pub async fn listen() -> io::Result<Listener> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = socket_path();
    ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name).map(Listener).map_err(
        |e| {
            if e.kind() == io::ErrorKind::PermissionDenied {
                io::Error::new(io::ErrorKind::AddrInUse, "Mirza is already running")
            } else {
                e
            }
        },
    )
}

#[cfg(windows)]
pub async fn serve(mut listener: Listener, tx: mpsc::UnboundedSender<Msg>) {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = socket_path();
    loop {
        if listener.0.connect().await.is_err() {
            continue;
        }
        let Ok(next) = ServerOptions::new().reject_remote_clients(true).create(&name) else { return };
        let connected = std::mem::replace(&mut listener.0, next);
        tokio::spawn(handle(connected, tx.clone()));
    }
}
