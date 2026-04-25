//! Unix-socket IPC server. Accepts newline-delimited JSON.
//!
//! NOT YET WIRED into main.rs — to start the server, call
//! `IpcServer::start(handle, socket_path)?` from the compositor's
//! event loop. That integration is a follow-up.

use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

use super::protocol::{PROTOCOL_VERSION, Request, Response};

/// Socket-server handle. Drop this to shut down the server thread.
pub struct IpcServer {
    socket_path: PathBuf,
    _thread: thread::JoinHandle<()>,
}

impl IpcServer {
    /// Start a server listening on `socket_path`. Spawns a background
    /// accept loop. Each connection is handled on a new thread.
    pub fn start<F>(
        socket_path: PathBuf,
        handler: F,
    ) -> Result<Self, Box<dyn Error + Send + Sync>>
    where
        F: Fn(Request) -> Response + Send + Sync + 'static + Clone,
    {
        // Remove stale socket if present
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        tracing::info!(path = %socket_path.display(), "zos-wm IPC server listening");

        let path_for_thread = socket_path.clone();
        let join = thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        let h = handler.clone();
                        thread::spawn(move || handle_client(s, h));
                    }
                    Err(e) => {
                        tracing::warn!(?e, "ipc accept error");
                        break;
                    }
                }
            }
            tracing::info!(path = %path_for_thread.display(), "zos-wm IPC server stopped");
        });

        Ok(Self {
            socket_path,
            _thread: join,
        })
    }

    /// Default socket path: $XDG_RUNTIME_DIR/zos-wm-$WAYLAND_DISPLAY.sock,
    /// or fall back to /tmp/zos-wm-$USER.sock.
    pub fn default_socket_path() -> PathBuf {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        if let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") {
            return PathBuf::from(rt).join(format!("zos-wm-{}.sock", display));
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        PathBuf::from(format!("/tmp/zos-wm-{}.sock", user))
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn handle_client<F>(stream: UnixStream, handler: F)
where
    F: Fn(Request) -> Response + Send + Sync + 'static,
{
    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(?e, "ipc clone stream failed");
            return;
        }
    };
    let reader = BufReader::new(read_stream);
    let mut writer = stream;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::debug!(?e, "ipc read error");
                break;
            }
        };
        if line.is_empty() {
            continue;
        }
        let req: Result<Request, _> = serde_json::from_str(&line);
        let resp = match req {
            Ok(Request::Quit) => break,
            Ok(r) => handler(r),
            Err(e) => Response::Error {
                message: format!("malformed request: {e}"),
            },
        };
        if let Ok(line) = serde_json::to_string(&resp) {
            if writeln!(writer, "{line}").is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    }
}

/// Build a default handler that returns an Error response indicating
/// "not yet integrated with compositor state". Real handlers wire to
/// AnvilState in main.rs.
pub fn placeholder_handler() -> impl Fn(Request) -> Response + Send + Sync + Clone + 'static {
    |req| match req {
        Request::Version => Response::Version {
            ipc: PROTOCOL_VERSION.into(),
            build: env!("CARGO_PKG_VERSION").into(),
        },
        _ => Response::Error {
            message: "IPC handler not yet wired to compositor state".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{Request, Response};

    #[test]
    fn protocol_request_roundtrip() {
        let cases = vec![
            Request::Workspaces {
                output: Some("DP-1".into()),
            },
            Request::Workspaces { output: None },
            Request::Windows {
                workspace: Some(3),
            },
            Request::Windows { workspace: None },
            Request::Monitors,
            Request::ActiveWindow,
            Request::SwitchToWorkspace { id: 2 },
            Request::MoveWindowToWorkspace { id: 7 },
            Request::FocusWindow { id: 42 },
            Request::CloseFocused,
            Request::Version,
            Request::Quit,
        ];
        for req in cases {
            let s = serde_json::to_string(&req).expect("serialize");
            let back: Request = serde_json::from_str(&s).expect("deserialize");
            assert_eq!(req, back, "round-trip mismatch on {req:?}");
        }
    }

    #[test]
    fn protocol_response_roundtrip() {
        let resp = Response::Version {
            ipc: "1".into(),
            build: "0.0.1".into(),
        };
        let s = serde_json::to_string(&resp).expect("serialize");
        let back: Response = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(resp, back);
    }

    #[test]
    fn placeholder_handler_returns_version() {
        let handler = placeholder_handler();
        let resp = handler(Request::Version);
        match resp {
            Response::Version { ipc, build } => {
                assert_eq!(ipc, PROTOCOL_VERSION);
                assert_eq!(build, env!("CARGO_PKG_VERSION"));
            }
            other => panic!("expected Version response, got {other:?}"),
        }
    }

    #[test]
    fn placeholder_handler_errors_on_unwired() {
        let handler = placeholder_handler();
        let resp = handler(Request::Monitors);
        match resp {
            Response::Error { message } => {
                assert!(message.contains("not yet wired"), "unexpected msg: {message}");
            }
            other => panic!("expected Error response, got {other:?}"),
        }
    }
}
