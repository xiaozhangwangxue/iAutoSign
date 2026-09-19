#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("../../locales", fallback = "en");

use crate::refresh::spawn_refresh_daemon;

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use single_instance::SingleInstance;

mod appearance;
mod certificate_reset;
mod defaults;
mod macos_app;
mod refresh;
mod relaunch;
mod screen;
mod startup;
mod subscriptions;
mod tray;

pub const APP_NAME: &str = "AutoSign";
pub const APP_NAME_VERSIONED: &str =
    concat!("自动签名助手 AutoSign - Version ", env!("CARGO_PKG_VERSION"));

fn main() -> iced::Result {
    init_logging();

    // Make local anisette generation possible before anything touches Apple's
    // servers.
    defaults::ensure_anisette_libraries();

    #[cfg(target_os = "windows")]
    check_apple_drivers();

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    if let Ok(locale) = current_locale::current_locale() {
        log::info!("Setting locale to {locale}...");
        rust_i18n::set_locale(&locale);
    }

    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    let _single_instance = match SingleInstance::new(&crate::relaunch::single_instance_key()) {
        Ok(instance) => {
            if !instance.is_single() {
                if let Err(err) = crate::relaunch::notify_running_instance() {
                    log::warn!("Failed to signal existing instance: {err}");
                }
                log::info!("Another instance is already running; exiting.");
                return Ok(());
            }
            Some(instance)
        }
        Err(err) => {
            log::warn!("Failed to acquire single-instance lock: {err}");
            None
        }
    };

    // For tray on linux.
    #[cfg(target_os = "linux")]
    {
        gtk::init().expect("GTK init failed");
    }

    // For notifications on macOS.
    #[cfg(target_os = "macos")]
    {
        notify_rust::get_bundle_identifier_or_default("Impactor");
        notify_rust::set_application("dev.khcrysalis.PlumeImpactor").ok();
    }

    let (_daemon_handle, connected_devices) = spawn_refresh_daemon();
    screen::set_refresh_daemon_devices(connected_devices);

    // We're going to try and try running the iced_daemon with different
    // environment variables so it can run properly
    // RE: https://github.com/claration/Impactor/issues/103, https://github.com/claration/Impactor/issues/90
    #[cfg(target_os = "linux")]
    check_gpu();

    run_daemon()
}

/// Writes the log next to the account data so that problems on a user's PC can
/// be diagnosed afterwards (a GUI build has no console to print to).
fn init_logging() {
    use std::fs::OpenOptions;

    let log_path = defaults::get_data_path().join("autosign.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);

    let builder = env_logger::Builder::new();
    let mut builder = builder;
    builder.filter_level(log::LevelFilter::Info);

    match file {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        Err(_) => {
            builder.target(env_logger::Target::Stderr);
        }
    }

    // RUST_LOG still wins when it is set explicitly.
    builder.parse_default_env();
    let _ = builder.try_init();

    log::info!("AutoSign {} starting", env!("CARGO_PKG_VERSION"));
    log::info!("Log file: {}", log_path.display());
}

/// On a fresh Windows PC the Apple device drivers may not be installed yet.
/// Without them nothing can see the phone, so tell the user instead of silently
/// showing "no device connected".
#[cfg(target_os = "windows")]
fn check_apple_drivers() {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };

    let reachable = runtime.block_on(async {
        match idevice::usbmuxd::UsbmuxdConnection::default().await {
            Ok(mut connection) => connection.get_devices().await.is_ok(),
            Err(_) => false,
        }
    });

    if reachable {
        return;
    }

    log::warn!("Apple Mobile Device Service is not reachable - drivers missing?");

    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("AutoSign - 缺少 Apple 手机驱动")
        .set_description(
            "未检测到 Apple 手机驱动（Apple Mobile Device Service）。\n\n\
             请重新运行 AutoSign 安装程序，保持“Apple 手机驱动与依赖”勾选；\
             或手动安装 iTunes 后再打开 AutoSign。\n\n\
             Apple Mobile Device Service was not found. Re-run the installer \
             with the driver option enabled, or install iTunes manually.",
        )
        .show();
}

#[cfg(target_os = "linux")]
fn check_gpu() {
    let instance = wgpu::Instance::default();

    let adapter =
        iced::futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }));

    match adapter {
        Ok(adapter) => {
            if !adapter.features().contains(wgpu::Features::SHADER_F16) {
                log::warn!("No FP16 support, falling back to tiny-skia");
                unsafe {
                    std::env::set_var("ICED_BACKEND", "tiny-skia");
                }
            }
        }
        Err(e) => {
            log::warn!("No adapter found: {e}, falling back to tiny-skia");
            unsafe {
                std::env::set_var("ICED_BACKEND", "tiny-skia");
            }
        }
    }
}

fn run_daemon() -> iced::Result {
    iced::daemon(
        screen::Impactor::new,
        screen::Impactor::update,
        screen::Impactor::view,
    )
    .subscription(screen::Impactor::subscription)
    .title(APP_NAME_VERSIONED)
    .theme(appearance::PlumeTheme::default().to_iced_theme())
    .settings(defaults::default_settings())
    .run()
}
