// Interface language row: a label and a drop-down of the supported languages.

use std::fmt;

use folia_core::{Language, LanguagePreference};
use folia_i18n::I18n;
use iced::widget::{pick_list, row, text};
use iced::{Alignment, Element};

use crate::Message;

#[derive(Debug, Clone, PartialEq)]
struct Choice {
    preference: LanguagePreference,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn view<'a>(i18n: &I18n, current: LanguagePreference) -> Element<'a, Message> {
    let choice = |preference| Choice { preference, label: i18n.t(label_key(preference)).to_owned() };
    let choices: Vec<Choice> = std::iter::once(LanguagePreference::System)
        .chain(Language::ALL.map(LanguagePreference::Fixed))
        .map(choice)
        .collect();

    row![
        text(i18n.t("options.appLanguage").to_owned()),
        pick_list(choices, Some(choice(current)), |picked: Choice| Message::LanguagePicked(picked.preference)),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}

fn label_key(preference: LanguagePreference) -> &'static str {
    match preference {
        LanguagePreference::System => "options.appLanguageSystem",
        LanguagePreference::Fixed(Language::En) => "options.appLanguageEnUS",
        LanguagePreference::Fixed(Language::ZhCn) => "options.appLanguageZhCN",
        LanguagePreference::Fixed(Language::ZhTw) => "options.appLanguageZhTW",
        LanguagePreference::Fixed(Language::In) => "options.appLanguageInID",
        LanguagePreference::Fixed(Language::Vi) => "options.appLanguageViVN",
    }
}
