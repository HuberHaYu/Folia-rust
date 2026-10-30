// Folia entry point: sets up logging, settings and text, then hands control to the UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use folia_i18n::I18n;
use folia_store::{AppDirs, AppearanceSettings, GeneralSettings, PlaybackSettings, Store};
use folia_ui::{Init, Shell};
use tracing_subscriber::EnvFilter;

// Debug builds keep their settings apart so development never touches a real install.
const APP_DIR: &str = if cfg!(debug_assertions) { "Folia-dev" } else { "Folia" };
const DEFAULT_LOG: &str = if cfg!(debug_assertions) { "debug,wgpu=warn,naga=warn,iced=info" } else { "info" };
const WINDOW_SIZE: (f32, f32) = (1200.0, 800.0);

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
        library_db: dirs.data.join("library.db"),
        cache_dir: dirs.cache.clone(),
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
        .window_size(WINDOW_SIZE)
        .run()?;
    Ok(())
}
