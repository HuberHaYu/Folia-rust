// Folia entry point: sets up logging, settings and text, then hands control to the UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use folia_i18n::I18n;
use folia_store::{AppDirs, AppearanceSettings, GeneralSettings, PlaybackSettings, PlayerSettings, Store};
use folia_ui::{Init, Shell};
use tracing_subscriber::EnvFilter;

// Debug builds keep their data apart from release ones, and both stay apart from the Electron app's "Folia" folder.
const APP_DIR: &str = if cfg!(debug_assertions) { "Folia-rust-dev" } else { "Folia-rust" };
const DEFAULT_LOG: &str =
    if cfg!(debug_assertions) { "debug,wgpu=warn,naga=warn,iced=info,cosmic_text=warn" } else { "info" };
const WINDOW_SIZE: iced::Size = iced::Size::new(1200.0, 800.0);

fn window_settings() -> anyhow::Result<iced::window::Settings> {
    let image =
        image::load_from_memory_with_format(include_bytes!("../../../assets/icon.png"), image::ImageFormat::Png)?
            .into_rgba8();
    let (width, height) = image.dimensions();
    let icon = iced::window::icon::from_rgba(image.into_raw(), width, height)?;
    Ok(iced::window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(iced::Size::new(350.0, 100.0)),
        icon: Some(icon),
        decorations: false,
        platform_specific: platform_window(),
        ..Default::default()
    })
}

#[cfg(target_os = "windows")]
fn platform_window() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific {
        undecorated_shadow: true,
        corner_preference: iced::window::settings::platform::CornerPreference::Round,
        ..Default::default()
    }
}

#[cfg(not(target_os = "windows"))]
fn platform_window() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific::default()
}

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_env("FOLIA_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let dirs = AppDirs::new(APP_DIR)?;
    let store = Store::at(&dirs.config);
    let general: GeneralSettings = store.load_or_default();
    let system_locale = sys_locale::get_locale();
    let i18n = I18n::new(general.language.resolve(system_locale.as_deref()))?;
    tracing::info!(config = %store.dir().display(), language = i18n.language().code(), "starting");

    let init = Init {
        appearance: store.load_or_default::<AppearanceSettings>(),
        playback: store.load_or_default::<PlaybackSettings>(),
        player: store.load_or_default::<PlayerSettings>(),
        library_db: dirs.data.join("library.db"),
        cache_dir: dirs.cache.clone(),
        keyring_service: APP_DIR.to_owned(),
        open_files: std::env::args_os().skip(1).map(std::path::PathBuf::from).collect(),
        store,
        general,
        i18n,
        system_locale,
    };
    iced::application(move || Shell::new(init.clone()), Shell::update, Shell::view)
        .title(Shell::title)
        .theme(Shell::theme)
        .subscription(Shell::subscription)
        .window(window_settings()?)
        .run()?;
    Ok(())
}
