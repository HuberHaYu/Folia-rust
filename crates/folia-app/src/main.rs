// Folia entry point: sets up logging, settings and text, then hands control to the UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use folia_i18n::I18n;
use folia_store::{GeneralSettings, Store};
use folia_ui::Shell;
use tracing_subscriber::EnvFilter;

// Debug builds keep their settings apart so development never touches a real install.
const APP_DIR: &str = if cfg!(debug_assertions) { "Folia-dev" } else { "Folia" };
const DEFAULT_LOG: &str = if cfg!(debug_assertions) { "debug,wgpu=warn,naga=warn,iced=info" } else { "info" };
const WINDOW_SIZE: (f32, f32) = (1200.0, 800.0);

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_env("FOLIA_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let store = Store::open(APP_DIR)?;
    let settings = store.load::<GeneralSettings>().unwrap_or_else(|err| {
        tracing::warn!(error = &err as &dyn std::error::Error, "general settings unreadable, using defaults");
        GeneralSettings::default()
    });
    let system_locale = sys_locale::get_locale();
    let i18n = I18n::new(settings.language.resolve(system_locale.as_deref()))?;
    tracing::info!(config = %store.dir().display(), language = i18n.language().code(), "starting");

    iced::application(
        move || Shell::new(store.clone(), settings.clone(), i18n.clone(), system_locale.clone()),
        Shell::update,
        Shell::view,
    )
    .title(Shell::title)
    .window_size(WINDOW_SIZE)
    .run()?;
    Ok(())
}
