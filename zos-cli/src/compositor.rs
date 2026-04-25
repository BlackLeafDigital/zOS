// === compositor.rs — IPC client for zos-wm ===
//
// Client for zos-wm's Unix-socket IPC.
//
// NOTE: the protocol types here mirror those in
// `zos-wm/src/ipc/protocol.rs`. Keep them in sync. Future work:
// move to a shared crate (e.g., `zos-core::ipc`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Request {
    Workspaces { output: Option<String> },
    Windows { workspace: Option<u32> },
    Monitors,
    ActiveWindow,
    SwitchToWorkspace { id: u32 },
    MoveWindowToWorkspace { id: u32 },
    FocusWindow { id: u32 },
    CloseFocused,
    Version,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Response {
    Workspaces { workspaces: Vec<Workspace> },
    Windows { windows: Vec<Window> },
    Monitors { monitors: Vec<Monitor> },
    ActiveWindow { window: Option<Window> },
    Ok,
    Error { message: String },
    Version { ipc: String, build: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: u32,
    pub output: String,
    pub windows: usize,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub id: u32,
    pub workspace_id: u32,
    pub output: String,
    pub class: String,
    pub title: String,
    pub focused: bool,
    pub band: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub active_workspace: Option<u32>,
}

/// Default IPC socket path. Mirrors `zos-wm/src/ipc/server.rs::default_socket_path`.
pub fn default_socket_path() -> PathBuf {
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    if let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(rt).join(format!("zos-wm-{}.sock", display));
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!("/tmp/zos-wm-{}.sock", user))
}

/// Connect to the socket, send `req`, read one response, return it.
pub fn send(req: &Request) -> Result<Response, Box<dyn std::error::Error>> {
    let path = default_socket_path();
    let stream = UnixStream::connect(&path)
        .map_err(|e| format!("failed to connect to {}: {}", path.display(), e))?;

    let mut writer = stream.try_clone().map_err(|e| format!("clone stream: {e}"))?;
    let req_line = serde_json::to_string(req)?;
    writeln!(writer, "{}", req_line)?;
    writer.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;
    let response: Response = serde_json::from_str(response_line.trim())
        .map_err(|e| format!("failed to parse response: {} (raw: {:?})", e, response_line))?;
    Ok(response)
}

// --- Watch helper ---

/// Loop running `f` every `interval_ms`, clearing the screen each time.
/// Runs until Ctrl-C / SIGTERM (process kill).
///
/// Errors from `f` are printed but do not crash the loop — useful when
/// zos-wm restarts and the IPC socket transiently disappears.
pub fn watch_loop<F>(interval_ms: u64, mut f: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut() -> Result<(), Box<dyn std::error::Error>>,
{
    use std::io::Write;
    let interval = std::time::Duration::from_millis(interval_ms);
    loop {
        // ANSI: clear screen + move cursor home
        print!("\x1b[2J\x1b[H");
        let _ = std::io::stdout().flush();
        if let Err(e) = f() {
            eprintln!("(watch) error: {}", e);
        }
        std::thread::sleep(interval);
    }
}

// --- Command handlers ---

pub fn cmd_version() -> Result<(), Box<dyn std::error::Error>> {
    match send(&Request::Version)? {
        Response::Version { ipc, build } => {
            println!("zos-wm IPC v{} (build {})", ipc, build);
        }
        Response::Error { message } => return Err(message.into()),
        other => return Err(format!("unexpected response: {:?}", other).into()),
    }
    Ok(())
}

pub fn cmd_workspaces(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let resp = send(&Request::Workspaces { output: None })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    match resp {
        Response::Workspaces { workspaces } => {
            println!("{:>4}  {:<14}  {:>5}  active", "id", "output", "wins");
            for w in workspaces {
                println!(
                    "{:>4}  {:<14}  {:>5}  {}",
                    w.id,
                    w.output,
                    w.windows,
                    if w.active { "*" } else { "" }
                );
            }
        }
        Response::Error { message } => return Err(message.into()),
        other => return Err(format!("unexpected response: {:?}", other).into()),
    }
    Ok(())
}

pub fn cmd_windows(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let resp = send(&Request::Windows { workspace: None })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    match resp {
        Response::Windows { windows } => {
            println!(
                "{:>6}  {:>4}  {:<14}  focused  {:<14}  class",
                "id", "ws", "output", "band"
            );
            for w in windows {
                println!(
                    "{:>6}  {:>4}  {:<14}  {:<7}  {:<14}  {}",
                    w.id,
                    w.workspace_id,
                    w.output,
                    if w.focused { "*" } else { "" },
                    w.band,
                    w.class
                );
                if !w.title.is_empty() {
                    println!("        {}", w.title);
                }
            }
        }
        Response::Error { message } => return Err(message.into()),
        other => return Err(format!("unexpected response: {:?}", other).into()),
    }
    Ok(())
}

pub fn cmd_monitors(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let resp = send(&Request::Monitors)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    match resp {
        Response::Monitors { monitors } => {
            println!(
                "{:>4}  {:<14}  {:>10}  {:>8}  active_ws",
                "id", "name", "resolution", "refresh"
            );
            for m in monitors {
                let res = format!("{}x{}", m.width, m.height);
                let refresh = format!("{:.1} Hz", m.refresh_mhz as f64 / 1000.0);
                let active_ws = m
                    .active_workspace
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".into());
                println!(
                    "{:>4}  {:<14}  {:>10}  {:>8}  {}",
                    m.id, m.name, res, refresh, active_ws
                );
            }
        }
        Response::Error { message } => return Err(message.into()),
        other => return Err(format!("unexpected response: {:?}", other).into()),
    }
    Ok(())
}

pub fn cmd_active(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let resp = send(&Request::ActiveWindow)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    match resp {
        Response::ActiveWindow { window } => match window {
            Some(w) => println!(
                "{} (id={}, ws={}, class={})",
                if w.title.is_empty() {
                    "(untitled)"
                } else {
                    &w.title
                },
                w.id,
                w.workspace_id,
                w.class
            ),
            None => println!("(no active window)"),
        },
        Response::Error { message } => return Err(message.into()),
        other => return Err(format!("unexpected response: {:?}", other).into()),
    }
    Ok(())
}

pub fn cmd_switch(id: u32) -> Result<(), Box<dyn std::error::Error>> {
    match send(&Request::SwitchToWorkspace { id })? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message.into()),
        other => Err(format!("unexpected response: {:?}", other).into()),
    }
}

pub fn cmd_close_focused() -> Result<(), Box<dyn std::error::Error>> {
    match send(&Request::CloseFocused)? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message.into()),
        other => Err(format!("unexpected response: {:?}", other).into()),
    }
}

pub fn cmd_move_to_workspace(id: u32) -> Result<(), Box<dyn std::error::Error>> {
    match send(&Request::MoveWindowToWorkspace { id })? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message.into()),
        other => Err(format!("unexpected response: {:?}", other).into()),
    }
}

pub fn cmd_focus_window(id: u32) -> Result<(), Box<dyn std::error::Error>> {
    match send(&Request::FocusWindow { id })? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message.into()),
        other => Err(format!("unexpected response: {:?}", other).into()),
    }
}
