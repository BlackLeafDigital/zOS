static POSSIBLE_BACKENDS: &[&str] = &[
    #[cfg(feature = "winit")]
    "--winit : Run zos-wm as a X11 or Wayland client using winit.",
    #[cfg(feature = "udev")]
    "--tty-udev : Run zos-wm as a tty udev client (requires root if without logind).",
    #[cfg(feature = "x11")]
    "--x11 : Run zos-wm as an X11 client.",
];

#[cfg(feature = "profile-with-tracy-mem")]
#[global_allocator]
static GLOBAL: profiling::tracy_client::ProfiledAllocator<std::alloc::System> =
    profiling::tracy_client::ProfiledAllocator::new(std::alloc::System, 10);

// Allow in this function because of existing usage
#[allow(clippy::uninlined_format_args)]
fn main() {
    // NVIDIA 555+ libgbm picks up the nvidia-drm backend natively when
    // nvidia_drm.modeset=1 is set. An inherited `GBM_BACKEND=nvidia-drm`
    // env var (zOS's hyprland.conf sets this for clients) can break the
    // compositor's own GBM init. Strip it before EGL/GBM touches anything.
    //
    // SAFETY: no other thread exists yet at this point in main.
    unsafe {
        std::env::remove_var("GBM_BACKEND");
    }

    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().compact().init();
    }

    #[cfg(feature = "profile-with-tracy")]
    profiling::tracy_client::Client::start();

    profiling::register_thread!("Main Thread");

    #[cfg(feature = "profile-with-puffin")]
    let _server = puffin_http::Server::new(&format!("0.0.0.0:{}", puffin_http::DEFAULT_PORT)).unwrap();
    #[cfg(feature = "profile-with-puffin")]
    profiling::puffin::set_scopes_on(true);

    let arg = ::std::env::args().nth(1);
    match arg.as_ref().map(|s| &s[..]) {
        #[cfg(feature = "winit")]
        Some("--winit") => {
            tracing::info!("Starting zos-wm with winit backend");
            zos_wm::winit::run_winit();
        }
        #[cfg(feature = "udev")]
        Some("--tty-udev") => {
            tracing::info!("Starting zos-wm on a tty using udev");
            zos_wm::udev::run_udev();
        }
        #[cfg(feature = "x11")]
        Some("--x11") => {
            tracing::info!("Starting zos-wm with x11 backend");
            zos_wm::x11::run_x11();
        }
        Some(other) => {
            tracing::error!("Unknown backend: {}", other);
        }
        None => {
            #[allow(clippy::disallowed_macros)]
            {
                println!("USAGE: zos-wm --backend");
                println!();
                println!("Possible backends are:");
                for b in POSSIBLE_BACKENDS {
                    println!("\t{b}");
                }
            }
        }
    }
}
