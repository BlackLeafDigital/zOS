//! zos-notify — DBus notification daemon for zOS.
//!
//! Implements `org.freedesktop.Notifications` on the session bus.
//! v1 scope: accept notifications, log them, store in-memory (cap 50).
//! v2: toast popup + history panel (separate task).

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::signal;
use tracing_subscriber::EnvFilter;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;
use zbus::{connection::Builder, interface};

/// One stored notification. Fields beyond `id` are not yet read in v1 — they
/// exist so v2 (history panel + toast renderer) can consume them without
/// changing the storage schema.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Notification {
    id:       u32,
    app_name: String,
    summary:  String,
    body:     String,
    icon:     String,
    actions:  Vec<String>,
    timeout:  i32,
}

/// Daemon state — guarded by `Mutex` because the zbus interface methods take
/// `&self` and may be called concurrently from the executor.
struct NotificationDaemon {
    notifications: Mutex<Vec<Notification>>,
    next_id:       Mutex<u32>,
}

/// Maximum number of in-memory notifications retained. Beyond this we drop the
/// oldest (FIFO) so a misbehaving app can't OOM the daemon.
const MAX_NOTIFICATIONS: usize = 50;

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    /// Notify(app_name, replaces_id, app_icon, summary, body, actions, hints,
    /// expire_timeout) -> u32
    ///
    /// See: <https://specifications.freedesktop.org/notification-spec/latest/protocol.html>
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        _hints: HashMap<String, Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            let mut n = self.next_id.lock().expect("next_id mutex poisoned");
            let id = *n;
            // 0 is reserved by spec — wrap to 1, never produce 0.
            *n = match n.checked_add(1) {
                Some(x) if x != 0 => x,
                _ => 1,
            };
            id
        };

        let notif = Notification {
            id,
            app_name: app_name.clone(),
            summary: summary.clone(),
            body: body.clone(),
            icon: app_icon.clone(),
            actions: actions.clone(),
            timeout: expire_timeout,
        };

        tracing::info!(
            id,
            app = %app_name,
            summary = %summary,
            body = %body,
            timeout = expire_timeout,
            "received notification"
        );

        let mut list = self
            .notifications
            .lock()
            .expect("notifications mutex poisoned");
        // If replacing, remove the old entry first.
        list.retain(|n| n.id != id);
        list.push(notif);
        // Cap at MAX_NOTIFICATIONS — drop oldest.
        while list.len() > MAX_NOTIFICATIONS {
            list.remove(0);
        }
        id
    }

    /// CloseNotification(id) — drop by id, emit `NotificationClosed`.
    async fn close_notification(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
    ) -> zbus::fdo::Result<()> {
        {
            let mut list = self
                .notifications
                .lock()
                .expect("notifications mutex poisoned");
            list.retain(|n| n.id != id);
        }
        // Reason 3 = closed via call to CloseNotification.
        Self::notification_closed(&emitter, id, 3).await?;
        Ok(())
    }

    /// GetCapabilities — what we support.
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".into(),
            "actions".into(),
            "icon-static".into(),
            "persistence".into(),
        ]
    }

    /// GetServerInformation -> (name, vendor, version, spec_version)
    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "zos-notify".into(),
            "zOS".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    /// NotificationClosed signal — emitted when a notification is dismissed.
    ///
    /// `reason` per spec:
    ///   1 = expired, 2 = dismissed by user, 3 = closed via CloseNotification,
    ///   4 = undefined.
    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// ActionInvoked signal — emitted when the user clicks a notification
    /// action. Not yet emitted in v1 (no toast UI), but defined so clients can
    /// subscribe without errors.
    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    tracing::info!("zos-notify starting");

    let daemon = NotificationDaemon {
        notifications: Mutex::new(Vec::new()),
        next_id:       Mutex::new(1),
    };

    let _connection = Builder::session()?
        .name("org.freedesktop.Notifications")?
        .serve_at("/org/freedesktop/Notifications", daemon)?
        .build()
        .await?;

    tracing::info!("zos-notify ready on session bus");

    // Block until SIGINT/SIGTERM.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = signal::ctrl_c() => tracing::info!("SIGINT received"),
        _ = sigterm.recv() => tracing::info!("SIGTERM received"),
    }
    tracing::info!("zos-notify shutting down");
    Ok(())
}
