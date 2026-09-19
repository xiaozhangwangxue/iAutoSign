use std::{
    env, fs,
    path::{Path, PathBuf},
};

use iced::window;

use crate::appearance;

pub(crate) fn default_settings() -> iced::Settings {
    iced::Settings {
        default_font: appearance::p_font(),
        default_text_size: appearance::THEME_FONT_SIZE.into(),
        fonts: appearance::load_fonts(),
        ..Default::default()
    }
}

pub(crate) fn default_window_settings() -> window::Settings {
    #[cfg(target_os = "macos")]
    let platform_specific = window::settings::PlatformSpecific {
        titlebar_transparent: true,
        title_hidden: true,
        fullsize_content_view: true,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    let platform_specific = window::settings::PlatformSpecific {
        application_id: String::from("dev.khcrysalis.PlumeImpactor"),
        ..Default::default()
    };

    #[cfg(target_os = "windows")]
    let platform_specific = window::settings::PlatformSpecific::default();

    window::Settings {
        // Sized to fit the installer screen (the tallest) without scrolling.
        size: iced::Size::new(575.0, 475.0),
        position: window::Position::Centered,
        exit_on_close_request: false,
        resizable: false,
        icon: Some(load_window_icon()),
        platform_specific: platform_specific,
        ..Default::default()
    }
}

fn load_window_icon() -> window::Icon {
    let bytes = include_bytes!(
        "../../../package/linux/icons/hicolor/64x64/apps/dev.khcrysalis.PlumeImpactor.png"
    );
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("Failed to load icon bytes")
        .to_rgba8();
    let (width, height) = image.dimensions();
    window::icon::from_rgba(image.into_raw(), width, height).unwrap()
}

pub fn get_data_path() -> PathBuf {
    let base = if cfg!(windows) {
        env::var("APPDATA").unwrap()
    } else {
        env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            env::var("HOME").unwrap() + "/.config"
        })
    };

    let dir = Path::new(&base).join("AutoSign");

    fs::create_dir_all(&dir).ok();

    dir
}

/// Copies the bundled anisette support libraries into the data directory.
///
/// Anisette data is what Apple uses to recognise "this computer". Generating it
/// locally needs Apple's Apple Music Android libraries, which the installer
/// ships in `libs/`. Copying them on first start means AutoSign does not depend
/// on a third party anisette server being online (that is the usual reason a
/// sign-in or renewal suddenly fails).
pub fn ensure_anisette_libraries() {
    let copied = plume_core::auth::install_bundled_anisette_libraries(get_data_path());
    if copied > 0 {
        log::info!("Installed {copied} bundled anisette library file(s)");
    }
}
