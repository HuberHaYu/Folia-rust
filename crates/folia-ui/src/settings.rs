// The settings window, after the original `SettingsModal`: a side list of categories, a page of option cards, search.

use std::time::Instant;

use folia_core::{Language, LanguagePreference, Theme};
use folia_i18n::I18n;
use folia_store::{AppearanceSettings, FrameRate, ThemeMode};
use folia_visualizer::{Ease, Style, tween};
use iced::widget::{
    button, column, container, mouse_area, opaque, operation, row, scrollable, space, stack, text, text_input,
};
use iced::{Alignment, Border, Color, Element, Length, Padding, Shadow, Size, Task, Vector};
use lucide_static_svg::Icon;

use crate::icons::icon;
use crate::settings_kit::{self as kit, Choice, Group, Row, Tokens};

/// `KIT_MOTION.move` and the window's own fade (`shellTransition`), in seconds.
const MOVE: f64 = 0.22;
const SHELL: f64 = 0.24;
/// `KIT_EASE.out`.
const KIT_OUT: Ease = Ease::new(0.22, 1.0, 0.36, 1.0);
const SIDE_WIDTH: f32 = 212.0;
const WIDE_WIDTH: f32 = 768.0;
const SCROLL: &str = "settings-page";
const THEMES: [ThemeMode; 2] = [ThemeMode::Light, ThemeMode::Dark];
const LIMITS: [FrameRate; 5] =
    [FrameRate::Fps30, FrameRate::Fps45, FrameRate::Fps60, FrameRate::Fps90, FrameRate::Fps120];
/// What switching the limit on picks, `FRAME_RATE_WHEN_SWITCHED_ON`.
const LIMIT_WHEN_SWITCHED_ON: FrameRate = FrameRate::Fps120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Player,
    General,
    Graphics,
}

impl Section {
    fn label_key(self) -> &'static str {
        match self {
            Section::Appearance => "options.settingsPageAppearance",
            Section::Player => "options.settingsPagePlayer",
            Section::General => "options.settingsPageGeneral",
            Section::Graphics => "options.settingsPageGraphics",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Appearance => Icon::Contrast,
            Section::Player => Icon::MonitorPlay,
            Section::General => Icon::SlidersHorizontal,
            Section::Graphics => Icon::Gauge,
        }
    }
}

/// The side list's groups, `SETTINGS_NAV_GROUP_SPECS` without the categories Folia-rust has nothing on yet.
const NAV: [(&str, &[Section]); 2] = [
    ("options.settingsGroupInterface", &[Section::Appearance, Section::Player]),
    ("options.settingsGroupApp", &[Section::General, Section::Graphics]),
];

/// A group of one page: its anchor id, heading and whether it is drawn without the heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupDef {
    id: &'static str,
    title: &'static str,
    bare: bool,
}

const LYRICS: GroupDef = GroupDef { id: "lyricsRenderer", title: "options.lyricsRenderer", bare: true };
const THEME: GroupDef = GroupDef { id: "themePresets", title: "options.settingsHeadingTheme", bare: false };
const LANGUAGE: GroupDef = GroupDef { id: "languageSettings", title: "options.languageSettings", bare: true };
const BAR: GroupDef = GroupDef { id: "bottomUiSettings", title: "options.settingsHeadingControlBar", bare: false };
const PERFORMANCE: GroupDef =
    GroupDef { id: "graphicsPerformance", title: "options.labPerformanceSection", bare: false };

/// Every option the pages draw, in page order: what the search finds.
const OPTIONS: [(Section, GroupDef, &str); 7] = [
    (Section::Appearance, LYRICS, "options.lyricsAnimationAdjust"),
    (Section::Appearance, THEME, "options.daylightMode"),
    (Section::Player, BAR, "options.playerBottomBarOffset"),
    (Section::Player, BAR, "options.playerBottomBarReposition"),
    (Section::General, LANGUAGE, "options.appLanguage"),
    (Section::Graphics, PERFORMANCE, "options.visualizerFrameRate"),
    (Section::Graphics, PERFORMANCE, "options.visualizerFrameRateValue"),
];

#[derive(Debug, Clone)]
pub enum Message {
    Section(Section),
    Query(String),
    /// A search hit: the category, and the group to land on.
    Hit(Section, Option<&'static str>),
    Tab(&'static str),
    Scrolled(f32),
    Escape,
    Close,
    Frame,
}

/// Parts whose mark glides to a new place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Theme,
    Limit,
    LimitValue,
    Tab,
}

#[derive(Debug, Clone, Copy)]
struct Motion {
    control: Control,
    from: f32,
    to: f32,
    at: Instant,
}

/// What the pages show and change; the shell owns the values.
pub struct Values<'a> {
    pub language: LanguagePreference,
    pub resolved: Language,
    pub appearance: &'a AppearanceSettings,
    pub style: Option<Style>,
    /// `playerBottomBarOffset` and its limit for this window.
    pub bar_offset: f32,
    pub bar_max: f32,
    /// A song is loaded, so the bar is on screen to be dragged.
    pub has_track: bool,
}

pub struct Settings {
    section: Section,
    query: String,
    tab: Option<&'static str>,
    opened: Instant,
    closing: Option<Instant>,
    page_at: Instant,
    motions: Vec<Motion>,
}

impl Settings {
    pub fn open() -> Self {
        let now = Instant::now();
        Self {
            section: Section::Appearance,
            query: String::new(),
            tab: None,
            opened: now,
            closing: None,
            page_at: now,
            motions: Vec::new(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        match message {
            Message::Section(section) => {
                self.show(section, None, now);
                self.scroll_to(None)
            }
            Message::Hit(section, group) => {
                self.query.clear();
                self.show(section, group, now);
                self.scroll_to(group)
            }
            Message::Query(query) => {
                self.query = query;
                self.page_at = now;
                Task::none()
            }
            Message::Tab(id) => {
                self.land(id, now);
                self.scroll_to(Some(id))
            }
            Message::Scrolled(offset) => {
                self.follow_scroll(offset, now);
                Task::none()
            }
            Message::Escape if !self.query.is_empty() => {
                self.query.clear();
                Task::none()
            }
            Message::Escape | Message::Close => {
                self.closing.get_or_insert(now);
                Task::none()
            }
            Message::Frame => {
                self.motions.retain(|motion| seconds(motion.at, now) < MOVE);
                Task::none()
            }
        }
    }

    /// Glides the marks of whatever the change moved.
    pub fn changed(&mut self, before: &AppearanceSettings, after: &AppearanceSettings) {
        let now = Instant::now();
        let theme_index = |mode: ThemeMode| THEMES.iter().position(|m| *m == mode).unwrap_or(0) as f32;
        let limited = |rate: FrameRate| if rate.fps().is_some() { 1.0 } else { 0.0 };
        let limit_index = |rate: FrameRate| LIMITS.iter().position(|r| *r == rate).map(|i| i as f32);
        if before.theme != after.theme {
            self.start(Control::Theme, theme_index(before.theme), theme_index(after.theme), now);
        }
        if limited(before.frame_rate) != limited(after.frame_rate) {
            self.start(Control::Limit, limited(before.frame_rate), limited(after.frame_rate), now);
        }
        if let (Some(from), Some(to)) = (limit_index(before.frame_rate), limit_index(after.frame_rate)) {
            self.start(Control::LimitValue, from, to, now);
        }
    }

    /// The window has faded out and can be dropped.
    pub fn finished(&self) -> bool {
        self.closing.is_some_and(|at| seconds(at, Instant::now()) >= SHELL)
    }

    /// Something is moving, so the shell must keep sending frames.
    pub fn animating(&self) -> bool {
        let now = Instant::now();
        seconds(self.opened, now) < SHELL
            || self.closing.is_some()
            || seconds(self.page_at, now) < MOVE
            || self.motions.iter().any(|motion| seconds(motion.at, now) < MOVE)
    }

    /// Scrolls the page so `group`'s gap sits at the top, or to the top of the page.
    fn scroll_to(&self, group: Option<&'static str>) -> Task<Message> {
        let groups = groups_of(self.section);
        let before = groups.iter().take_while(|g| Some(g.id) != group).count();
        let before = if group.is_some() && before < groups.len() { before } else { 0 };
        let y: f32 =
            groups[..before].iter().enumerate().map(|(index, g)| group_height(self.section, g, index == 0)).sum();
        operation::scroll_to(SCROLL, operation::AbsoluteOffset { x: None, y: Some(y) })
    }

    fn show(&mut self, section: Section, group: Option<&'static str>, now: Instant) {
        if section != self.section || group.is_none() {
            self.page_at = now;
            self.motions.retain(|motion| motion.control != Control::Tab);
        }
        self.section = section;
        self.tab = group;
    }

    fn land(&mut self, id: &'static str, now: Instant) {
        let groups = groups_of(self.section);
        let index = |id: Option<&str>| groups.iter().position(|g| Some(g.id) == id).unwrap_or(0) as f32;
        let from = self.at(Control::Tab, index(self.tab), now);
        self.start(Control::Tab, from, index(Some(id)), now);
        self.tab = Some(id);
    }

    /// Moves the tab line to the group whose card the page has scrolled to.
    fn follow_scroll(&mut self, offset: f32, now: Instant) {
        let groups = groups_of(self.section);
        let mut top = 0.0;
        let mut current = groups.first().map(|g| g.id);
        for (index, group) in groups.iter().enumerate() {
            if top > offset + 1.0 {
                break;
            }
            current = Some(group.id);
            top += group_height(self.section, group, index == 0);
        }
        if let Some(id) = current.filter(|id| Some(*id) != self.tab.or(groups.first().map(|g| g.id))) {
            self.land(id, now);
        }
    }

    fn start(&mut self, control: Control, from: f32, to: f32, now: Instant) {
        let from = self.at(control, from, now);
        self.motions.retain(|motion| motion.control != control);
        self.motions.push(Motion { control, from, to, at: now });
    }

    /// Where `control`'s mark is drawn now; `rest` when it is not moving.
    fn at(&self, control: Control, rest: f32, now: Instant) -> f32 {
        let Some(motion) = self.motions.iter().find(|motion| motion.control == control) else { return rest };
        tween(f64::from(motion.from), f64::from(motion.to), seconds(motion.at, now), MOVE, KIT_OUT) as f32
    }

    /// How far the window has come in (1) or gone out (0).
    fn shown(&self, now: Instant) -> f32 {
        let opening = tween(0.0, 1.0, seconds(self.opened, now), SHELL, Ease::OUT);
        let closing = self.closing.map_or(1.0, |at| tween(1.0, 0.0, seconds(at, now), SHELL, Ease::OUT));
        opening.min(closing) as f32
    }

    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        theme: &Theme,
        daylight: bool,
        window: Size,
        values: Values<'a>,
    ) -> Element<'a, crate::Message> {
        let now = Instant::now();
        let shown = self.shown(now);
        let tokens = Tokens::new(theme, daylight).faded(shown);
        let wide = window.width >= WIDE_WIDTH;
        let page_in = tween(0.0, 1.0, seconds(self.page_at, now), MOVE, KIT_OUT) as f32;

        let side = self.side_list(i18n, tokens, wide);
        let body = column![
            self.header(i18n, tokens, wide),
            self.content(i18n, tokens.faded(page_in), page_in, &values, wide, now)
        ]
        .width(Length::Fill)
        .height(Length::Fill);
        let inner: Element<'a, crate::Message> = if wide {
            row![
                side,
                container(space()).width(1).height(Length::Fill).style(move |_| kit::rounded(tokens.line, 0.0)),
                body
            ]
            .into()
        } else {
            column![side, kit::hairline(tokens), body].into()
        };

        let (pad_x, pad_y) = if window.width >= 640.0 { (20.0, 48.0) } else { (16.0, 32.0) };
        let width = if wide { 960.0_f32.min(window.width - pad_x * 2.0) } else { window.width - pad_x * 2.0 };
        let height = window.height * if wide { 0.86 } else { 0.9 };
        let shadow_alpha = 0.5 * shown;
        let panel = container(inner).width(width).height(height).style(move |_| container::Style {
            background: Some(tokens.win.into()),
            border: Border { color: tokens.line2, width: 1.0, radius: 16.0.into() },
            shadow: Shadow {
                color: Color { a: shadow_alpha, ..Color::BLACK },
                offset: Vector::new(0.0, 36.0),
                blur_radius: 90.0,
            },
            text_color: Some(tokens.t1),
            ..container::Style::default()
        });
        let lift = 20.0 * (1.0 - shown);
        let panel = container(opaque(panel))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding { top: pad_y + lift, bottom: (pad_y - lift).max(0.0), left: pad_x, right: pad_x })
            .center_x(Length::Fill)
            .center_y(Length::Fill);
        let veil_alpha = if daylight { 0.32 } else { 0.5 } * shown;
        let backdrop = container(space()).width(Length::Fill).height(Length::Fill).style(move |_| container::Style {
            background: Some(Color { a: veil_alpha, ..Color::BLACK }.into()),
            ..container::Style::default()
        });
        let backdrop = opaque(mouse_area(backdrop).on_press(crate::Message::Settings(Message::Close)));
        stack![backdrop, panel].into()
    }

    fn side_list<'a>(&'a self, i18n: &'a I18n, tokens: Tokens, wide: bool) -> Element<'a, crate::Message> {
        let search = self.search_field(i18n, tokens);
        let active = self.query.is_empty().then_some(self.section);
        if !wide {
            let strip =
                NAV.iter().flat_map(|(_, sections)| sections.iter()).map(|s| nav_item(i18n, tokens, *s, active, false));
            let strip = scrollable(row(strip).spacing(4))
                .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::hidden()))
                .width(Length::Fill);
            return container(column![search, space().height(8), strip])
                .padding(Padding { top: 12.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(tokens.side.into()),
                    border: Border {
                        radius: iced::border::Radius::default().top_left(16.0).top_right(16.0),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
                .into();
        }
        let mut nav = column![space().height(4)];
        for (group_key, sections) in NAV {
            let label = text(i18n.t(group_key)).size(12).color(tokens.t3);
            nav = nav.push(container(label).padding(Padding { top: 14.0, right: 10.0, bottom: 4.0, left: 10.0 }));
            let items = sections.iter().map(|section| nav_item(i18n, tokens, *section, active, true));
            nav = nav.push(column(items).spacing(1));
        }
        container(column![search, nav])
            .width(SIDE_WIDTH)
            .height(Length::Fill)
            .padding(Padding { top: 14.0, right: 10.0, bottom: 10.0, left: 10.0 })
            .style(move |_| container::Style {
                background: Some(tokens.side.into()),
                border: Border {
                    radius: iced::border::Radius::default().top_left(16.0).bottom_left(16.0),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    }

    fn search_field<'a>(&'a self, i18n: &'a I18n, tokens: Tokens) -> Element<'a, crate::Message> {
        let input = text_input(i18n.t("options.settingsSearchPlaceholder"), &self.query)
            .on_input(|query| crate::Message::Settings(Message::Query(query)))
            .size(13)
            .line_height(iced::widget::text::LineHeight::Absolute(18.0.into()))
            .padding(Padding { top: 6.0, right: 30.0, bottom: 6.0, left: 32.0 })
            .style(move |_, status| text_input::Style {
                background: tokens.fill.into(),
                border: Border {
                    color: if matches!(status, text_input::Status::Focused { .. }) {
                        tokens.line2
                    } else {
                        Color::TRANSPARENT
                    },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                icon: tokens.t3,
                placeholder: tokens.t3,
                value: tokens.t1,
                selection: Color { a: 0.3 * tokens.accent.a, ..tokens.accent },
            });
        let glass = container(icon(Icon::Search, 14.0, tokens.t3)).padding(Padding::ZERO.left(10.0)).center_y(32);
        let mut field = stack![input, glass];
        if !self.query.is_empty() {
            let clear = button(icon(Icon::X, 14.0, tokens.t3))
                .padding(0)
                .style(|_, _| button::Style::default())
                .on_press(crate::Message::Settings(Message::Query(String::new())));
            field = field.push(
                container(clear)
                    .width(Length::Fill)
                    .align_x(Alignment::End)
                    .padding(Padding::ZERO.right(10.0))
                    .center_y(32),
            );
        }
        field.into()
    }

    fn header<'a>(&'a self, i18n: &'a I18n, tokens: Tokens, wide: bool) -> Element<'a, crate::Message> {
        let gutter = Gutter::of(wide);
        let searching = !self.query.is_empty();
        let title = if searching { i18n.t("options.settingsSearchTitle") } else { i18n.t(self.section.label_key()) };
        let groups = groups_of(self.section);
        let has_tabs = !searching && groups.len() > 1;
        let close = kit::icon_button(tokens, Icon::X, 16.0, crate::Message::Settings(Message::Close));
        let title =
            text(title).size(20).line_height(iced::widget::text::LineHeight::Absolute(32.0.into())).font(kit::SEMIBOLD);
        let plain = !searching && !has_tabs;
        let top = row![title, space().width(Length::Fill), close].align_y(Alignment::Center).padding(Padding {
            top: gutter.title_top,
            right: gutter.close_right,
            bottom: if plain { gutter.title_top - 4.0 } else { 0.0 },
            left: gutter.side,
        });
        if searching {
            let hits = search(i18n, &self.query).len();
            let summary = if hits > 0 {
                i18n.format("options.settingsSearchCount", &[("n", &hits.to_string())])
            } else {
                i18n.t("options.settingsSearchEmpty").to_owned()
            };
            let summary = container(text(summary).size(13).color(tokens.t2)).height(36).align_y(Alignment::Center);
            return column![
                top,
                container(summary).padding(Padding {
                    top: gutter.under_title,
                    left: gutter.side,
                    right: gutter.side,
                    bottom: 0.0
                }),
                kit::hairline(tokens)
            ]
            .into();
        }
        if !has_tabs {
            return column![top, kit::hairline(tokens)].into();
        }
        let chosen = groups.iter().position(|g| Some(g.id) == self.tab).unwrap_or(0);
        let mark = self.at(Control::Tab, chosen as f32, Instant::now());
        let entries =
            groups.iter().map(|g| (i18n.t(g.title).to_owned(), crate::Message::Settings(Message::Tab(g.id)))).collect();
        column![top, kit::tabs(tokens, entries, Some(chosen), Some(mark), gutter.under_title + 4.0, gutter.side)].into()
    }

    fn content<'a>(
        &'a self,
        i18n: &'a I18n,
        tokens: Tokens,
        page_in: f32,
        values: &Values<'a>,
        wide: bool,
        now: Instant,
    ) -> Element<'a, crate::Message> {
        let rise = 6.0 * (1.0 - page_in);
        let body: Element<'a, crate::Message> = if self.query.is_empty() {
            let groups = self.page(i18n, tokens, values, now);
            column(groups.into_iter().enumerate().map(|(index, group)| kit::group(tokens, group, index == 0))).into()
        } else {
            search_results(i18n, tokens, &self.query)
        };
        let gutter = Gutter::of(wide);
        let body = container(body).padding(Padding {
            top: rise,
            right: gutter.side,
            bottom: gutter.foot - rise,
            left: gutter.side,
        });
        scrollable(body)
            .id(SCROLL)
            .height(Length::Fill)
            .direction(crate::settings_kit::scrollbar())
            .on_scroll(|viewport| crate::Message::Settings(Message::Scrolled(viewport.absolute_offset().y)))
            .style(crate::settings_kit::scroll_style(tokens))
            .into()
    }

    fn page<'a>(
        &'a self,
        i18n: &'a I18n,
        tokens: Tokens,
        values: &Values<'a>,
        now: Instant,
    ) -> Vec<Group<'a, crate::Message>> {
        let group = |def: GroupDef, rows| Group { title: i18n.t(def.title).to_owned(), bare: def.bare, rows };
        let row = |label: &str, hint: Option<String>, sub: bool, control| Row {
            label: i18n.t(label).to_owned(),
            hint,
            sub,
            control,
        };
        match self.section {
            Section::Appearance => {
                let styles: Vec<_> =
                    Style::ALL.into_iter().map(Some).chain([None]).map(|s| style_choice(i18n, s)).collect();
                let style =
                    kit::select(tokens, styles, style_choice(i18n, values.style), |c: Choice<Option<Style>>| {
                        crate::Message::Visualizer(c.value)
                    });
                let themes = THEMES.into_iter().map(|mode| (mode, i18n.t(theme_key(mode)).to_owned())).collect();
                let theme_rest = THEMES.iter().position(|m| *m == values.appearance.theme).unwrap_or(0) as f32;
                let mark = self.at(Control::Theme, theme_rest, now);
                let daylight =
                    kit::segmented(tokens, themes, values.appearance.theme, Some(mark), crate::Message::Theme);
                vec![
                    group(
                        LYRICS,
                        vec![row(
                            "options.lyricsAnimationAdjust",
                            Some(i18n.t("options.lyricsRendererDesc").to_owned()),
                            false,
                            style,
                        )],
                    ),
                    group(THEME, vec![row("options.daylightMode", None, false, daylight)]),
                ]
            }
            Section::Player => {
                // `PlayerPage` control bar: the height with its reset, and dragging the bar on the player page.
                let base = folia_store::BOTTOM_BAR_BASE;
                let reset = if values.bar_offset > base {
                    kit::icon_button(tokens, Icon::RotateCcw, 15.0, crate::Message::BarOffsetReset)
                } else {
                    container(icon(Icon::RotateCcw, 15.0, Color { a: tokens.t2.a * 0.4, ..tokens.t2 }))
                        .center(32)
                        .into()
                };
                let slider = iced::widget::slider(
                    base..=values.bar_max.max(base + 1.0),
                    values.bar_offset,
                    crate::Message::BarOffset,
                )
                .on_release(crate::Message::BarOffsetDone)
                .step(1.0_f32)
                .width(Length::Fixed(160.0))
                .style(move |_, _| kit::slider_style(tokens));
                let height =
                    iced::widget::row![reset, slider, kit::value(tokens, format!("{}px", values.bar_offset.round()))]
                        .spacing(8)
                        .align_y(iced::Alignment::Center);
                let reposition = if values.has_track {
                    kit::icon_button(tokens, Icon::ChevronRight, 16.0, crate::Message::StartPositioning)
                } else {
                    container(icon(Icon::ChevronRight, 16.0, Color { a: tokens.t2.a * 0.4, ..tokens.t2 }))
                        .center(32)
                        .into()
                };
                let unavailable =
                    (!values.has_track).then(|| i18n.t("options.playerBottomBarRepositionUnavailable").to_owned());
                vec![group(
                    BAR,
                    vec![
                        row(
                            "options.playerBottomBarOffset",
                            Some(i18n.t("options.playerBottomBarOffsetDesc").to_owned()),
                            false,
                            height.into(),
                        ),
                        row("options.playerBottomBarReposition", unavailable, false, reposition),
                    ],
                )]
            }
            Section::General => {
                let languages: Vec<_> = std::iter::once(LanguagePreference::System)
                    .chain(Language::ALL.map(LanguagePreference::Fixed))
                    .map(|p| language_choice(i18n, p))
                    .collect();
                let hint = (values.language == LanguagePreference::System).then(|| {
                    let current = i18n.t(language_key(LanguagePreference::Fixed(values.resolved)));
                    i18n.format("options.appLanguageSystemHint", &[("language", current)])
                });
                let picker = kit::select(
                    tokens,
                    languages,
                    language_choice(i18n, values.language),
                    |c: Choice<LanguagePreference>| crate::Message::LanguagePicked(c.value),
                );
                vec![group(LANGUAGE, vec![row("options.appLanguage", hint, false, picker)])]
            }
            Section::Graphics => {
                let rate = values.appearance.frame_rate;
                let limited = rate.fps().is_some();
                let next = if limited { FrameRate::Display } else { LIMIT_WHEN_SWITCHED_ON };
                let switch = kit::toggle(
                    tokens,
                    self.at(Control::Limit, if limited { 1.0 } else { 0.0 }, now),
                    crate::Message::FrameRate(next),
                );
                let mut rows = vec![row(
                    "options.visualizerFrameRate",
                    Some(i18n.t("options.visualizerFrameRateDesc").to_owned()),
                    false,
                    switch,
                )];
                if let Some(index) = LIMITS.iter().position(|r| *r == rate) {
                    let choices =
                        LIMITS.into_iter().map(|r| (r, format!("{} FPS", r.fps().unwrap_or_default()))).collect();
                    let mark = self.at(Control::LimitValue, index as f32, now);
                    rows.push(row(
                        "options.visualizerFrameRateValue",
                        None,
                        true,
                        kit::segmented(tokens, choices, rate, Some(mark), crate::Message::FrameRate),
                    ));
                }
                vec![group(PERFORMANCE, rows)]
            }
        }
    }
}

/// The page's spacing: `md:` values from 768px up, the narrower ones below.
struct Gutter {
    side: f32,
    title_top: f32,
    close_right: f32,
    under_title: f32,
    foot: f32,
}

impl Gutter {
    fn of(wide: bool) -> Self {
        if wide {
            Self { side: 32.0, title_top: 20.0, close_right: 16.0, under_title: 4.0, foot: 64.0 }
        } else {
            Self { side: 16.0, title_top: 16.0, close_right: 12.0, under_title: 0.0, foot: 48.0 }
        }
    }
}

/// The groups of a category, in page order.
fn groups_of(section: Section) -> Vec<GroupDef> {
    let mut groups: Vec<GroupDef> = Vec::new();
    for (owner, group, _) in OPTIONS {
        if owner == section && !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

/// A group's height on the page, for following the scroll with the tab line: hint rows are a line taller.
fn group_height(section: Section, group: &GroupDef, first: bool) -> f32 {
    let rows = OPTIONS.iter().filter(|(owner, g, _)| *owner == section && g == group).count() as f32;
    let gap = if first { kit::FIRST_GROUP_GAP } else { kit::GROUP_GAP };
    let heading = if group.bare { 0.0 } else { kit::GROUP_HEADING };
    gap + heading + rows * kit::ROW_HINT_HEIGHT
}

/// One search hit: where it lands, what it is called and where it sits.
struct Hit {
    section: Section,
    group: Option<&'static str>,
    label: String,
    path: String,
}

/// `buildSettingsSearchEntries` matched against `query`: categories, their groups and options, in side-list order.
fn search(i18n: &I18n, query: &str) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    let mut entries = Vec::new();
    for (nav_key, sections) in NAV {
        for section in sections.iter().copied() {
            let page = i18n.t(section.label_key());
            entries.push(Hit { section, group: None, label: page.to_owned(), path: i18n.t(nav_key).to_owned() });
            let mut listed: Vec<&str> = Vec::new();
            for (_, group, label) in OPTIONS.iter().filter(|(owner, ..)| *owner == section) {
                let title = i18n.t(group.title);
                if !listed.contains(&group.id) {
                    listed.push(group.id);
                    entries.push(Hit {
                        section,
                        group: Some(group.id),
                        label: title.to_owned(),
                        path: page.to_owned(),
                    });
                }
                entries.push(Hit {
                    section,
                    group: Some(group.id),
                    label: i18n.t(label).to_owned(),
                    path: format!("{page} \u{203a} {title}"),
                });
            }
        }
    }
    entries.retain(|hit| hit.label.to_lowercase().contains(&query));
    entries
}

fn search_results<'a>(i18n: &'a I18n, tokens: Tokens, query: &str) -> Element<'a, crate::Message> {
    let hits = search(i18n, query);
    if hits.is_empty() {
        return space().into();
    }
    let mut card = column![];
    for (index, hit) in hits.into_iter().enumerate() {
        if index > 0 {
            card = card.push(container(kit::hairline(tokens)).padding(Padding::ZERO.left(16.0)));
        }
        let line = row![
            text(hit.label).size(14).color(tokens.t1).width(Length::Fill),
            kit::value(tokens, hit.path),
            icon(Icon::ChevronRight, 16.0, tokens.t2)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let link = button(container(line).height(56).align_y(Alignment::Center).padding([0, 16]))
            .padding(0)
            .width(Length::Fill)
            .on_press(crate::Message::Settings(Message::Hit(hit.section, hit.group)))
            .style(move |_, status| match status {
                button::Status::Hovered | button::Status::Pressed => kit::rounded_button(tokens.fill, 0.0),
                _ => button::Style::default(),
            });
        card = card.push(link);
    }
    let card = container(card).width(Length::Fill).clip(true).style(move |_| container::Style {
        background: Some(tokens.card.into()),
        border: Border { color: tokens.line, width: 1.0, radius: 12.0.into() },
        ..container::Style::default()
    });
    column![space().height(kit::FIRST_GROUP_GAP), card].into()
}

fn nav_item<'a>(
    i18n: &'a I18n,
    tokens: Tokens,
    section: Section,
    active: Option<Section>,
    wide: bool,
) -> Element<'a, crate::Message> {
    let current = active == Some(section);
    let color = if current { tokens.t1 } else { tokens.t2 };
    let label = text(i18n.t(section.label_key())).size(13).color(color);
    let content: Element<'a, crate::Message> = if wide {
        let mark = if current { color } else { Color { a: color.a * 0.7, ..color } };
        row![icon(section.icon(), 16.0, mark), label].spacing(10).align_y(Alignment::Center).into()
    } else {
        label.into()
    };
    let item = button(container(content).height(32).align_y(Alignment::Center).padding([0, 10]))
        .padding(0)
        .on_press(crate::Message::Settings(Message::Section(section)))
        .style(move |_, status| {
            if current {
                kit::rounded_button(tokens.fill2, 8.0)
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                kit::rounded_button(tokens.fill, 8.0)
            } else {
                button::Style::default()
            }
        });
    if wide { item.width(Length::Fill).into() } else { item.into() }
}

fn style_choice(i18n: &I18n, style: Option<Style>) -> Choice<Option<Style>> {
    Choice { value: style, label: i18n.t(style.map_or("ui.visualizerRandom", Style::label_key)).to_owned() }
}

fn language_choice(i18n: &I18n, preference: LanguagePreference) -> Choice<LanguagePreference> {
    Choice { value: preference, label: i18n.t(language_key(preference)).to_owned() }
}

fn theme_key(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "options.daylightModeLight",
        ThemeMode::Dark => "options.daylightModeDark",
    }
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

fn seconds(since: Instant, now: Instant) -> f64 {
    now.saturating_duration_since(since).as_secs_f64()
}
