// Full-screen search from `SearchWorkspace`: a tinted page, one field, the platform in use, then the track list.

use std::time::Instant;

use folia_core::Track;
use folia_i18n::I18n;
use folia_omni::Omni;
use folia_visualizer::Ease;
use iced::widget::scrollable::Viewport;
use iced::widget::{button, column, container, row, space, stack, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Task, Theme};

use crate::background::error_chain;
use crate::icons::icon;
use crate::track_list::TrackList;
use lucide_static_svg::Icon;

const PAGE_SIZE: u32 = 50;
const FADE_MS: f64 = 200.0;
const FADE_EASE: Ease = Ease::new(0.0, 0.0, 0.2, 1.0);

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    Submit,
    Results(Result<Vec<Track>, String>),
    Scrolled(Viewport),
    Play(usize),
    Close,
    Frame,
}

pub struct SearchPage {
    query: String,
    pub results: Vec<Track>,
    busy: bool,
    submitted: bool,
    error: bool,
    list: TrackList,
    fade_from: f32,
    fade_goal: f32,
    fade_at: Instant,
}

impl Default for SearchPage {
    fn default() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            busy: false,
            submitted: false,
            error: false,
            list: TrackList::default(),
            fade_from: 0.0,
            fade_goal: 0.0,
            fade_at: Instant::now(),
        }
    }
}

impl SearchPage {
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn reveal(&mut self) {
        let now = Instant::now();
        self.fade_from = self.fade(now);
        self.fade_goal = 1.0;
        self.fade_at = now;
    }

    pub fn close(&mut self) {
        let now = Instant::now();
        self.fade_from = self.fade(now);
        self.fade_goal = 0.0;
        self.fade_at = now;
    }

    pub fn animating(&self, now: Instant) -> bool {
        (self.fade_from - self.fade_goal).abs() > 0.001
            && now.saturating_duration_since(self.fade_at).as_secs_f64() * 1000.0 < FADE_MS
    }

    pub fn gone(&self, now: Instant) -> bool {
        self.fade_goal == 0.0 && !self.animating(now) && self.fade(now) < 0.02
    }

    pub fn fade(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.fade_at).as_secs_f64() * 1000.0 / FADE_MS).clamp(0.0, 1.0);
        self.fade_from + (self.fade_goal - self.fade_from) * FADE_EASE.at(t) as f32
    }

    /// Handles a page message; returns the index of a result to play when one was chosen.
    pub fn update(
        &mut self,
        message: Message,
        omni: Option<&Omni>,
        provider: &'static str,
    ) -> (Task<Message>, Option<usize>) {
        match message {
            Message::Query(query) => self.query = query,
            Message::Submit => {
                let query = self.query.trim().to_owned();
                let Some(omni) = omni.cloned() else { return (Task::none(), None) };
                if query.is_empty() || self.busy {
                    return (Task::none(), None);
                }
                self.busy = true;
                self.submitted = true;
                self.error = false;
                let search = async move {
                    omni.search(provider, &query, PAGE_SIZE, 0).await.map(|page| page.items).map_err(|err| {
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
            Message::Close | Message::Frame => {}
        }
        (Task::none(), None)
    }

    pub fn view<'a>(&'a self, view: View<'a>, now: Instant) -> Element<'a, Message> {
        let shown = self.fade(now);
        let narrow = view.width < 640.0;
        let (pad_x, pad_y) = if narrow { (12.0, 16.0) } else { (24.0, 24.0) };
        let shift = 28.0 * (1.0 - shown);
        let ink = Color { a: view.text.a * shown, ..view.text };
        let field = self.field(view, ink, shown);
        let close = close_button(ink, view.daylight, shown);
        let source = source_pill(view, shown);
        let body = self.results(view, ink);
        let header = column![row![field, close].spacing(12).align_y(Alignment::Center), source].spacing(12);
        let page = column![header, body]
            .spacing(12)
            .width(Length::Fixed(view.width.min(1024.0)))
            .padding(iced::Padding { top: pad_y + shift, right: pad_x, bottom: pad_y, left: pad_x });
        let page = container(page).width(Length::Fill).align_x(Alignment::Center);
        let veil = if view.daylight {
            Color { r: 250.0 / 255.0, g: 250.0 / 255.0, b: 250.0 / 255.0, a: 0.96 * shown }
        } else {
            Color { r: 8.0 / 255.0, g: 8.0 / 255.0, b: 10.0 / 255.0, a: 0.94 * shown }
        };
        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(veil)),
                ..container::Style::default()
            })
            .into()
    }

    fn field<'a>(&'a self, view: View<'a>, ink: Color, shown: f32) -> Element<'a, Message> {
        let border = if view.daylight {
            Color { a: 0.10 * shown, ..Color::BLACK }
        } else {
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.10 * shown }
        };
        let fill = if view.daylight {
            Color { a: 0.04 * shown, ..Color::BLACK }
        } else {
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.05 * shown }
        };
        let input = text_input(view.i18n.t("search.placeholder"), &self.query)
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .size(14)
            .padding(iced::Padding { top: 14.0, right: 16.0, bottom: 14.0, left: 44.0 })
            .style(move |_theme: &Theme, _status| text_input::Style {
                background: Background::Color(fill),
                border: Border { color: border, width: 1.0, radius: 16.0.into() },
                icon: ink,
                placeholder: Color { a: 0.45 * ink.a, ..ink },
                value: ink,
                selection: Color { a: 0.3, ..view.accent },
            });
        let mark = if self.busy { Icon::Loader } else { Icon::Search };
        let glyph = container(icon(mark, 16.0, Color { a: 0.45 * ink.a, ..ink }))
            .padding(iced::Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 16.0 })
            .center_y(Length::Fill);
        stack![input, glyph].into()
    }

    fn results<'a>(&'a self, view: View<'a>, ink: Color) -> Element<'a, Message> {
        if self.busy && self.results.is_empty() {
            return container(text(view.i18n.t("ui.searching")).size(14).color(Color { a: 0.5 * ink.a, ..ink }))
                .center(Length::Fill)
                .height(Length::Fill)
                .into();
        }
        if self.error && self.results.is_empty() {
            let retry = button(text(view.i18n.t("search.retry")).size(14).color(ink))
                .padding([8, 16])
                .on_press(Message::Submit)
                .style(button::text);
            return container(
                column![text(view.i18n.t("search.error")).size(14).color(ink), retry]
                    .spacing(12)
                    .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .height(Length::Fill)
            .into();
        }
        if !self.submitted {
            return space().height(Length::Fill).into();
        }
        if self.results.is_empty() {
            return container(text(view.i18n.t("home.noResults")).size(14).color(Color { a: 0.5 * ink.a, ..ink }))
                .center(Length::Fill)
                .height(Length::Fill)
                .into();
        }
        self.list.view(
            &self.results,
            view.i18n,
            Color { a: 0.6 * ink.a, ..ink },
            view.playing,
            Message::Scrolled,
            Message::Play,
        )
    }
}

#[derive(Clone, Copy)]
pub struct View<'a> {
    pub i18n: &'a I18n,
    pub text: Color,
    pub background: Color,
    pub accent: Color,
    pub daylight: bool,
    pub width: f32,
    pub playing: Option<i64>,
    /// The short name of the platform searched.
    pub source: &'static str,
}

fn source_pill<'a>(view: View<'a>, shown: f32) -> Element<'a, Message> {
    let label = text(view.source).size(12).font(medium()).color(view.background);
    button(label)
        .padding([8, 16])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(Background::Color(Color { a: shown, ..view.accent })),
            border: Border { radius: 999.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .into()
}

fn close_button<'a>(ink: Color, daylight: bool, shown: f32) -> Element<'a, Message> {
    let fill = if daylight {
        Color { a: 0.05 * shown, ..Color::BLACK }
    } else {
        Color { r: 1.0, g: 1.0, b: 1.0, a: 0.10 * shown }
    };
    button(icon(Icon::X, 20.0, ink))
        .padding(12)
        .on_press(Message::Close)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(Color { a: if hot { fill.a + 0.05 } else { fill.a }, ..fill })),
                border: Border { radius: 999.0.into(), ..Border::default() },
                ..button::Style::default()
            }
        })
        .into()
}

fn medium() -> iced::Font {
    crate::typeface::font(iced::font::Weight::Medium)
}
