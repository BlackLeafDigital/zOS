//! zos-notify — DBus notification daemon + toast popup UI.
//!
//! Implements `org.freedesktop.Notifications` on the session bus and
//! renders a stack of layer-shell toast popups in the top-right corner
//! whenever a notification arrives.
//!
//! ## Architecture
//!
//! Two runtimes cooperate:
//!
//! - A dedicated OS thread hosts a multi-thread tokio runtime that owns
//!   the zbus connection. zbus tasks run there; the connection lives for
//!   the lifetime of the process.
//! - The main thread runs `iced_layershell::application(...).run()` which
//!   builds its own tokio runtime internally (via `iced_futures` tokio
//!   backend) for the wayland event loop.
//!
//! The DBus side communicates with the iced side via an unbounded mpsc
//! channel. iced's `update` drains the channel each tick (250 ms). This
//! is single-threaded inside iced (`update` is `&mut`), so we keep the
//! receiver behind an `Arc<StdMutex<...>>` only because `update`'s state
//! type must be owned and we want to share construction between the
//! `boot` closure and any future re-mount.
//!
//! ## Spec compliance
//!
//! - `Notify` returns the assigned id; `replaces_id` is honored.
//! - `expire_timeout`: positive ms → auto-dismiss after that many ms.
//!   `0` → daemon-default of 5 s (per spec, "the notification's expiration
//!   time is dependent on the notification server's settings").
//!   `-1` → sticky (never auto-dismiss).
//! - `CloseNotification` removes a toast immediately.
//! - `GetCapabilities` advertises `body`, `icon-static`, `persistence`.
//! - `GetServerInformation` reports `("zos-notify","zOS",VERSION,"1.2")`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use iced::{
    Background, Border, Element, Length, Subscription, Task, Theme,
    border::Radius,
    widget::{column, container, mouse_area, text},
};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;
use zbus::{connection::Builder, interface};
use zos_ui::theme;

// --- Visual / layout constants -------------------------------------------

const TOAST_W: u32 = 360;
const TOAST_H: u32 = 84;
const TOAST_GAP: u32 = 8;
const MAX_VISIBLE_TOASTS: usize = 4;
const SURFACE_PADDING: u32 = 12;
const TICK_MS: u64 = 250;
const DEFAULT_EXPIRE_MS: u64 = 5_000;

// --- Domain types --------------------------------------------------------

/// A single live toast. `expires_at == None` means sticky (`expire_timeout
/// == -1`); otherwise the toast is dropped from the visible stack the
/// first tick after `Instant::now() >= expires_at`.
#[derive(Debug, Clone)]
struct ToastNotification {
    id:         u32,
    summary:    String,
    body:       String,
    #[allow(dead_code)]
    icon:       String,
    expires_at: Option<Instant>,
}

/// Events the DBus side pushes into the iced UI side.
#[derive(Debug, Clone)]
enum DaemonEvent {
    Show(ToastNotification),
    Close(u32),
}

// --- DBus service --------------------------------------------------------

/// Daemon state. zbus method handlers take `&self`, so all interior
/// mutability lives here. The mpsc sender is cloneable (cheap), so we
/// just hold it.
struct NotificationDaemon {
    next_id: StdMutex<u32>,
    tx:      mpsc::UnboundedSender<DaemonEvent>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    /// `Notify(app_name, replaces_id, app_icon, summary, body, actions,
    ///         hints, expire_timeout) -> u32`
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
        _actions: Vec<String>,
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

        let expires_at = if expire_timeout > 0 {
            Some(Instant::now() + Duration::from_millis(expire_timeout as u64))
        } else if expire_timeout == 0 {
            // 0 = daemon-decided default per spec.
            Some(Instant::now() + Duration::from_millis(DEFAULT_EXPIRE_MS))
        } else {
            // -1 = sticky (no auto-dismiss).
            None
        };

        tracing::info!(
            id,
            app = %app_name,
            summary = %summary,
            body = %body,
            timeout = expire_timeout,
            "received notification"
        );

        // Best-effort delivery to UI. If the iced side is gone (process
        // shutting down), we still acknowledge the DBus call so clients
        // don't see errors.
        let _ = self.tx.send(DaemonEvent::Show(ToastNotification {
            id,
            summary,
            body,
            icon: app_icon,
            expires_at,
        }));
        id
    }

    /// `CloseNotification(id)` — drop by id, emit `NotificationClosed`.
    async fn close_notification(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
    ) -> zbus::fdo::Result<()> {
        let _ = self.tx.send(DaemonEvent::Close(id));
        // Reason 3 = closed via call to CloseNotification.
        Self::notification_closed(&emitter, id, 3).await?;
        Ok(())
    }

    /// `GetCapabilities` — what we support.
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".into(),
            "icon-static".into(),
            "persistence".into(),
        ]
    }

    /// `GetServerInformation -> (name, vendor, version, spec_version)`.
    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "zos-notify".into(),
            "zOS".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    /// `NotificationClosed` signal — emitted when a notification is dismissed.
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

    /// `ActionInvoked` signal — emitted when the user clicks a notification
    /// action. Not yet emitted in v1 (toast UI has no action buttons), but
    /// defined so clients can subscribe without errors.
    #[zbus(signal)]
    #[allow(dead_code)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

// --- iced UI -------------------------------------------------------------

/// iced-side state. The receiver is wrapped in `Arc<StdMutex<_>>` so the
/// `boot` closure can capture by clone; in practice only `update` ever
/// locks it, and only via `try_lock`, so the mutex is never contended.
struct UiState {
    toasts: Vec<ToastNotification>,
    rx:     Arc<StdMutex<mpsc::UnboundedReceiver<DaemonEvent>>>,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Msg {
    /// Fired ~4 Hz; drains the DBus channel and reaps expired toasts.
    Tick,
    /// User clicked a toast — dismiss it.
    DismissOne(u32),
}

fn make_boot(
    rx: Arc<StdMutex<mpsc::UnboundedReceiver<DaemonEvent>>>,
) -> impl Fn() -> (UiState, Task<Msg>) + 'static {
    move || {
        (
            UiState {
                toasts: Vec::new(),
                rx:     rx.clone(),
            },
            Task::none(),
        )
    }
}

fn namespace() -> String {
    "zos-notify".into()
}

fn theme_fn(_: &UiState) -> Theme {
    theme::zos_theme()
}

fn update(state: &mut UiState, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Tick => {
            // Drain pending DBus events. `try_lock` rather than `lock` so
            // a stuck mutex never blocks the UI thread; in practice
            // there's no contender.
            if let Ok(mut rx) = state.rx.try_lock() {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        DaemonEvent::Show(toast) => {
                            // Replace existing toast with the same id (so
                            // `replaces_id` works visually); otherwise
                            // append, capping the visible stack.
                            state.toasts.retain(|t| t.id != toast.id);
                            state.toasts.push(toast);
                            while state.toasts.len() > MAX_VISIBLE_TOASTS {
                                state.toasts.remove(0);
                            }
                        }
                        DaemonEvent::Close(id) => {
                            state.toasts.retain(|t| t.id != id);
                        }
                    }
                }
            }
            // Reap expired toasts.
            let now = Instant::now();
            state.toasts.retain(|t| match t.expires_at {
                Some(exp) => exp > now,
                None => true,
            });
        }
        Msg::DismissOne(id) => {
            state.toasts.retain(|t| t.id != id);
        }
        // Layer-shell control messages (anchor/size/margin/etc.) injected
        // by `#[to_layer_message]`. We don't reconfigure the surface at
        // runtime — initial `LayerShellSettings` describe it fully.
        _ => {}
    }
    Task::none()
}

fn view(state: &UiState) -> Element<'_, Msg> {
    if state.toasts.is_empty() {
        // Render an empty container — the layer-shell surface is sized
        // for up to MAX_VISIBLE_TOASTS, but has no painted content when
        // the stack is empty.
        return container(text("")).into();
    }

    let mut col = column![].spacing(TOAST_GAP as f32);
    for toast in &state.toasts {
        col = col.push(toast_view(toast));
    }

    container(col)
        .padding(theme::space::X2)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn toast_view<'a>(t: &ToastNotification) -> Element<'a, Msg> {
    let inner = column![
        text(t.summary.clone())
            .size(theme::font_size::BASE)
            .color(theme::TEXT),
        text(t.body.clone())
            .size(theme::font_size::SM)
            .color(theme::SUBTEXT0),
    ]
    .spacing(theme::space::X1);

    let card = container(inner)
        .padding(theme::space::X3)
        .width(Length::Fixed(TOAST_W as f32))
        .height(Length::Fixed(TOAST_H as f32))
        .style(toast_style);

    mouse_area(card).on_press(Msg::DismissOne(t.id)).into()
}

fn toast_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::SURFACE0)),
        text_color: Some(theme::TEXT),
        border: Border {
            color:  theme::SURFACE2,
            width:  1.0,
            radius: Radius::from(theme::radius::MD),
        },
        ..Default::default()
    }
}

fn subscription(_: &UiState) -> Subscription<Msg> {
    iced::time::every(Duration::from_millis(TICK_MS)).map(|_| Msg::Tick)
}

// --- DBus side runtime ---------------------------------------------------

/// Spawn a dedicated OS thread hosting a multi-thread tokio runtime that
/// owns the zbus session connection. The connection (and the daemon
/// object inside it) lives until the process exits.
///
/// The thread blocks on `pending::<()>().await` after registration so
/// the runtime stays alive — zbus drives its tasks in the background.
fn spawn_dbus_thread(tx: mpsc::UnboundedSender<DaemonEvent>) {
    std::thread::Builder::new()
        .name("zos-notify-dbus".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("zos-notify-dbus-worker")
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to build DBus tokio runtime");
                    return;
                }
            };

            rt.block_on(async move {
                let daemon = NotificationDaemon {
                    next_id: StdMutex::new(1),
                    tx,
                };

                let conn = match Builder::session()
                    .and_then(|b| b.name("org.freedesktop.Notifications"))
                    .and_then(|b| {
                        b.serve_at("/org/freedesktop/Notifications", daemon)
                    }) {
                    Ok(b) => match b.build().await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::error!(error = ?e, "zbus build() failed");
                            return;
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = ?e, "zbus Builder configuration failed");
                        return;
                    }
                };

                tracing::info!("zos-notify ready on session bus");

                // Hold the connection alive for the lifetime of this thread.
                // Without this, `conn` would drop and the bus name would
                // be released as soon as we returned.
                let _conn = conn;
                std::future::pending::<()>().await;
            });
        })
        .expect("failed to spawn DBus thread");
}

// --- Entrypoint ----------------------------------------------------------

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("zos-notify starting");

    let (tx, rx) = mpsc::unbounded_channel::<DaemonEvent>();
    let rx = Arc::new(StdMutex::new(rx));

    // DBus daemon owns its own runtime in a background OS thread so we
    // don't fight iced_layershell over executor ownership.
    spawn_dbus_thread(tx);

    // Layer-shell surface: anchored top-right, overlay layer, no exclusive
    // zone, sized to fit the maximum visible stack plus padding. Keyboard
    // is None (toasts are click-only).
    let layer_settings = LayerShellSettings {
        anchor: Anchor::Top | Anchor::Right,
        layer: Layer::Overlay,
        exclusive_zone: 0,
        size: Some((
            TOAST_W + SURFACE_PADDING * 2,
            (TOAST_H + TOAST_GAP) * MAX_VISIBLE_TOASTS as u32 + SURFACE_PADDING * 2,
        )),
        margin: (12, 12, 0, 0),
        keyboard_interactivity: KeyboardInteractivity::None,
        start_mode: StartMode::Active,
        events_transparent: false,
    };

    iced_layershell::application(make_boot(rx), namespace, update, view)
        .theme(theme_fn)
        .subscription(subscription)
        .settings(Settings {
            layer_settings,
            antialiasing: true,
            ..Default::default()
        })
        .run()
}
