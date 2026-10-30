// Search page: finds songs on an online platform through omni and lists them for playing.

use folia_core::Track;
use folia_i18n::I18n;
use folia_omni::Omni;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Color, Element, Length, Task};

use crate::background::error_chain;
use crate::track_list::TrackList;

const PAGE_SIZE: u32 = 50;
const PROVIDER: &str = "netease";

#[derive(Default)]
pub struct SearchPage {
    query: String,
    pub results: Vec<Track>,
    busy: bool,
    error: bool,
    list: TrackList,
}

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    Submit,
    Results(Result<Vec<Track>, String>),
    Scrolled(Viewport),
    Play(usize),
}

impl SearchPage {
    /// Handles a page message; returns the index of a result to play when one was chosen.
    pub fn update(&mut self, message: Message, omni: Option<&Omni>) -> (Task<Message>, Option<usize>) {
        match message {
            Message::Query(query) => self.query = query,
            Message::Submit => {
                let query = self.query.trim().to_owned();
                let Some(omni) = omni.cloned() else { return (Task::none(), None) };
                if query.is_empty() || self.busy {
                    return (Task::none(), None);
                }
                self.busy = true;
                let search = async move {
                    omni.search(PROVIDER, &query, PAGE_SIZE, 0).await.map(|page| page.items).map_err(|err| {
                        tracing::warn!(error = &err as &dyn std::error::Error, "search failed");
                        error_chain(&err)
                    })
                };
                return (Task::perform(search, Message::Results), None);
            }
            Message::Results(result) => {
                self.busy = false;
                self.error = result.is_err();
                self.results = result.unwrap_or_default();
                self.list.reset();
            }
            Message::Scrolled(viewport) => self.list.scrolled(viewport),
            Message::Play(index) => return (Task::none(), Some(index)),
        }
        (Task::none(), None)
    }

    pub fn view<'a>(&'a self, i18n: &'a I18n, muted: Color, playing: Option<i64>) -> Element<'a, Message> {
        let input = text_input(i18n.t("search.placeholder"), &self.query)
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .padding(10)
            .width(Length::Fill);
        let submit =
            button(text(i18n.t("libraryBravaisHome.searchSubmit"))).padding([10, 16]).on_press(Message::Submit);
        let header = row![input, submit].spacing(12).align_y(Alignment::Center);

        let body: Element<'a, Message> = if self.busy {
            centered(i18n.t("ui.searching"), muted)
        } else if self.error {
            centered(i18n.t("search.error"), muted)
        } else {
            self.list.view(&self.results, i18n, muted, playing, Message::Scrolled, Message::Play)
        };
        column![header, body].spacing(16).padding(24).into()
    }
}

fn centered(label: &str, muted: Color) -> Element<'_, Message> {
    container(text(label).color(muted)).center(Length::Fill).into()
}
