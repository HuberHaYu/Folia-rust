// Settings page: interface language, theme and the visualizer frame rate cap.

use folia_core::{Language, LanguagePreference};
use folia_i18n::I18n;
use folia_store::{AppearanceSettings, FrameRate, ThemeMode};
use iced::widget::{column, pick_list, row, text};
use iced::{Alignment, Color, Element, Length};

use crate::Message;

/// A drop-down entry whose label is already translated.
#[derive(Debug, Clone, PartialEq)]
struct Choice<T> {
    value: T,
    label: String,
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn view<'a>(
    i18n: &'a I18n,
    language: LanguagePreference,
    appearance: &AppearanceSettings,
    muted: Color,
) -> Element<'a, Message> {
    let theme_label = |mode: ThemeMode| Choice {
        value: mode,
        label: i18n
            .t(if mode == ThemeMode::Dark { "options.daylightModeDark" } else { "options.daylightModeLight" })
            .to_owned(),
    };
    let frame_label = |rate: FrameRate| Choice {
        value: rate,
        label: rate.fps().map_or_else(|| i18n.t("options.frameRateDisplay").to_owned(), |fps| format!("{fps} FPS")),
    };

    let language_label = |preference: LanguagePreference| Choice {
        value: preference,
        label: i18n.t(language_key(preference)).to_owned(),
    };
    let languages: Vec<_> = std::iter::once(LanguagePreference::System)
        .chain(Language::ALL.map(LanguagePreference::Fixed))
        .map(language_label)
        .collect();
    let themes: Vec<_> = [ThemeMode::Dark, ThemeMode::Light].into_iter().map(theme_label).collect();
    let rates: Vec<_> = FrameRate::ALL.into_iter().map(frame_label).collect();

    column![
        section(i18n.t("options.languageSettings"), muted),
        setting(
            i18n.t("options.appLanguage"),
            pick_list(languages, Some(language_label(language)), |choice: Choice<LanguagePreference>| {
                Message::LanguagePicked(choice.value)
            }),
        ),
        section(i18n.t("options.settingsPageAppearance"), muted),
        setting(
            i18n.t("options.settingsPageAppearance"),
            pick_list(themes, Some(theme_label(appearance.theme)), |choice: Choice<ThemeMode>| Message::Theme(
                choice.value
            )),
        ),
        setting(
            i18n.t("options.visualizerFrameRate"),
            pick_list(rates, Some(frame_label(appearance.frame_rate)), |choice: Choice<FrameRate>| {
                Message::FrameRate(choice.value)
            }),
        ),
    ]
    .spacing(16)
    .padding(24)
    .width(Length::Fill)
    .into()
}

fn section(title: &str, muted: Color) -> Element<'_, Message> {
    text(title.to_owned()).size(13).color(muted).into()
}

fn setting<'a>(label: &str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label.to_owned()).width(Length::Fixed(260.0)), control.into()]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
}

fn language_key(preference: LanguagePreference) -> &'static str {
    match preference {
        LanguagePreference::System => "options.appLanguageSystem",
        LanguagePreference::Fixed(Language::En) => "options.appLanguageEnUS",
        LanguagePreference::Fixed(Language::ZhCn) => "options.appLanguageZhCN",
        LanguagePreference::Fixed(Language::ZhTw) => "options.appLanguageZhTW",
        LanguagePreference::Fixed(Language::In) => "options.appLanguageInID",
        LanguagePreference::Fixed(Language::Vi) => "options.appLanguageViVN",
    }
}
