// Top-level UI state and the iced update/view functions that drive it.

mod language_picker;

use folia_core::LanguagePreference;
use folia_i18n::I18n;
use folia_store::{GeneralSettings, Store};
use iced::widget::{column, text};
use iced::{Element, Task};

#[derive(Debug, Clone)]
pub enum Message {
    LanguagePicked(LanguagePreference),
}

pub struct Shell {
    store: Store,
    settings: GeneralSettings,
    i18n: I18n,
    system_locale: Option<String>,
}

impl Shell {
    pub fn new(store: Store, settings: GeneralSettings, i18n: I18n, system_locale: Option<String>) -> Self {
        Self { store, settings, i18n, system_locale }
    }

    pub fn title(&self) -> String {
        self.i18n.t("app.name").to_owned()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LanguagePicked(preference) => self.set_language(preference),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        column![text(self.i18n.t("app.name")).size(32), language_picker::view(&self.i18n, self.settings.language)]
            .spacing(24)
            .padding(32)
            .into()
    }

    fn set_language(&mut self, preference: LanguagePreference) -> Task<Message> {
        let language = preference.resolve(self.system_locale.as_deref());
        if let Err(err) = self.i18n.set_language(language) {
            tracing::error!(error = &err as &dyn std::error::Error, "cannot switch language");
            return Task::none();
        }
        self.settings.language = preference;
        tracing::info!(language = language.code(), ?preference, "language switched");

        let store = self.store.clone();
        let settings = self.settings.clone();
        Task::future(async move {
            if let Err(err) = store.save(&settings) {
                tracing::error!(error = &err as &dyn std::error::Error, "cannot save general settings");
            }
        })
        .discard()
    }
}
