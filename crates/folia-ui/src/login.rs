// The shared sign-in window (`LoginHost`, `LoginScreen`, `LoginWindow`): platform column, code tile, message and steps.

use std::time::{Duration, Instant};

use folia_core::Rgb;
use folia_i18n::I18n;
use folia_omni::{Account, LoginQr, LoginState, Omni, OmniError, Platform};
use folia_visualizer::Ease;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, gradient};
use iced::widget::{button, canvas as canvas_widget, column, container, mouse_area, row, space, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Padding, Point, Rectangle, Renderer, Shadow, Size,
    Task, Theme, Vector, mouse,
};
use lucide_static_svg::Icon;

use crate::background::blocking;
use crate::icons::icon;
use crate::motion::Tween;
use crate::online::{AccountState, PROVIDER};
use crate::platform_icon::platform_icon;
use crate::settings_kit::Tokens;

/// `LoginWindow`: 772 wide, at least 420 high, a 212px column; Sheet keeps 16px round it.
const WIDTH: f32 = 772.0;
const MARGIN: f32 = 16.0;
const REST_HEIGHT: f32 = 420.0;
/// The open details' share of the window (`Unfold` under the tile and the message).
const DETAILS_HEIGHT: f32 = 140.0;
const ASIDE: f32 = 212.0;
const TILE: f32 = 200.0;
/// Where the tile's centre is in the wide window: rings leave from here.
const TILE_CENTER: Point = Point::new(ASIDE + 24.0 + TILE / 2.0, 130.0 + TILE / 2.0);
/// `SEARCH_FROM`: the column has a search field from this many platforms on.
const SEARCH_FROM: usize = 12;
/// `PROVIDER_LOGIN_POLL_INTERVAL_MS`, `PROVIDER_LOGIN_TRANSIENT_POLL_LIMIT`, `PROVIDER_LOGIN_SCANNED_LIMIT_MS`.
const POLL: Duration = Duration::from_millis(2000);
const TRANSIENT_LIMIT: u32 = 2;
const SCANNED_LIMIT: Duration = Duration::from_secs(60);
/// `KIT_MOTION` and `KIT_EASE`.
const UNFOLD_MS: f64 = 300.0;
const MOVE_MS: f64 = 220.0;
const EASE_OUT: Ease = Ease::new(0.22, 1.0, 0.36, 1.0);
const EASE_STANDARD: Ease = Ease::new(0.4, 0.0, 0.2, 1.0);
const EASE_IN_OUT: Ease = Ease::new(0.65, 0.0, 0.35, 1.0);
/// `LOGIN_DONE`: after the account is read, the flood leaves the tile, the window leaves, the end.
const DONE_FRONT: f64 = 110.0;
const DONE_LEAVE: f64 = 1260.0;
const DONE_END: f64 = 1800.0;
const SEMIBOLD: Font = crate::typeface::font(iced::font::Weight::Semibold);
const MEDIUM: Font = crate::typeface::font(iced::font::Weight::Medium);

/// `LoginCaseId`s a QR sign-in can end in, with their class (`LoginCaseKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    CodeExpired,
    ExpiredAfterScan,
    Unreachable,
    NoAnswer,
    Refused,
    RetryLater,
    AccountNotRead,
    Unknown,
}

impl Case {
    /// `LOGIN_CASE_TEXTS`.
    fn texts(self) -> (&'static str, Option<&'static str>) {
        match self {
            Case::CodeExpired => ("login.caseCodeExpired", None),
            Case::ExpiredAfterScan => ("login.caseNotConfirmed", Some("login.caseCodeExpired")),
            Case::Unreachable => ("login.caseUnreachable", Some("login.noteCheckNetwork")),
            Case::NoAnswer => ("login.caseNoAnswer", Some("login.noteCheckNetwork")),
            Case::Refused => ("login.caseRefused", Some("login.noteRetryLater")),
            Case::RetryLater => ("login.caseRetryLater", None),
            Case::AccountNotRead => ("login.caseAccountNotRead", None),
            Case::Unknown => ("login.caseUnknown", Some("login.noteUnknown")),
        }
    }

    /// `KIND_GLYPH`: A calm, B and D warn, C, F and G error.
    fn glyph(self) -> (Icon, Tone) {
        match self {
            Case::CodeExpired | Case::ExpiredAfterScan => (Icon::Clock, Tone::Calm),
            Case::Unreachable | Case::NoAnswer => (Icon::WifiOff, Tone::Warn),
            Case::Refused | Case::RetryLater => (Icon::Ban, Tone::Error),
            Case::AccountNotRead => (Icon::UserX, Tone::Error),
            Case::Unknown => (Icon::CircleQuestionMark, Tone::Error),
        }
    }

    fn expired_code(self) -> bool {
        matches!(self, Case::CodeExpired | Case::ExpiredAfterScan)
    }

    /// The case's id and `LOGIN_KIND_TEXTS` for its class, as the details' kind row writes them.
    fn kind(self) -> (&'static str, &'static str) {
        match self {
            Case::CodeExpired => ("code-expired", "login.kindUnfinished"),
            Case::ExpiredAfterScan => ("expired-after-scan", "login.kindUnfinished"),
            Case::Unreachable => ("unreachable", "login.kindUnreachable"),
            Case::NoAnswer => ("no-answer", "login.kindUnreachable"),
            Case::Refused => ("refused", "login.kindRefused"),
            Case::RetryLater => ("retry-later", "login.kindRefused"),
            Case::AccountNotRead => ("account-not-read", "login.kindAccount"),
            Case::Unknown => ("unknown", "login.kindUnknown"),
        }
    }

    /// `CASES[id].details`: every case but an expired code has details.
    fn details(self) -> bool {
        self != Case::CodeExpired
    }
}

/// `LoginStep`: where in a QR sign-in it stopped (`loginStepText`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Wait,
    Check,
    Account,
}

impl Step {
    fn text(self) -> &'static str {
        match self {
            Step::Start => "login.stepStartCode",
            Step::Wait => "login.stepWaitPhone",
            Step::Check => "login.stepCheck",
            Step::Account => "login.stepAccount",
        }
    }
}

/// `DEFAULT_RETRY_WAIT_MS`: how long 「稍后再试」 waits when the platform names no time.
const RETRY_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Calm,
    Warn,
    Error,
}

/// Why a sign-in did not finish, and what the platform said (`failureQuote`).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    case: Case,
    quote: Option<(String, Option<String>)>,
    step: Step,
    /// The platform's code, the HTTP status and the error's words, as the details' raw row writes them.
    raw: String,
    at: Instant,
}

impl Failure {
    fn of(case: Case, step: Step) -> Self {
        Self { case, quote: None, step, raw: String::new(), at: Instant::now() }
    }

    /// `retryOpensAt`: a wait-then-retry case opens its retry `RETRY_WAIT` after it failed.
    fn retry_opens(&self) -> Instant {
        if self.case == Case::RetryLater { self.at + RETRY_WAIT } else { self.at }
    }

    /// `caseIdOf` for an error thrown while starting or checking: HTTP 429, 5xx, 4xx, a network error, or the platform's code.
    fn from_error(err: &OmniError, step: Step) -> Self {
        let (case, quote) = match err {
            OmniError::Http(http) => {
                let case = match http.status().map(|status| status.as_u16()) {
                    Some(429) => Case::RetryLater,
                    Some(status) if status >= 500 => Case::Unreachable,
                    Some(_) => Case::Refused,
                    None if http.is_timeout() => Case::NoAnswer,
                    None => Case::Unreachable,
                };
                let code = http.status().map(|status| format!("HTTP {}", status.as_u16()));
                (case, code.map(|code| (code, None)))
            }
            OmniError::Platform(code) => (Case::Refused, Some((code.to_string(), None))),
            OmniError::Response(message) => (Case::Refused, Some((message.clone(), None))),
            _ => (Case::Unknown, Some((err.to_string(), None))),
        };
        Self { case, quote, step, raw: crate::background::error_chain(err), at: Instant::now() }
    }

    fn network(err: &OmniError) -> bool {
        matches!(err, OmniError::Http(http) if http.status().is_none())
    }
}

/// `LoginScene` for a QR sign-in.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Preparing,
    Code,
    Scanned,
    Reading,
    Failed(Failure),
}

#[derive(Debug, Clone)]
pub enum Message {
    Choose(&'static str),
    Hover(Option<&'static str>),
    /// The platform search above twelve rows or more (`SEARCH_FROM`).
    Query(String),
    Close,
    Retry,
    /// Opens or closes a failure's details.
    Details,
    /// The account's sign-out: asks first (`askProviderSignOut`).
    SignOut,
    ConfirmSignOut,
    CancelSignOut,
    Qr(u64, Result<LoginQr, Failure>),
    Poll(u64),
    Checked(u64, Result<LoginState, (Failure, bool)>),
    Frame,
    Swallow,
}

/// What the shell does after a window message.
pub enum Effect {
    /// The phone confirmed: keep the session and read the account.
    Confirmed,
    SignOut,
}

pub struct Login {
    shown: Tween,
    open: bool,
    chosen: Option<&'static str>,
    round: u64,
    phase: Phase,
    qr: Option<LoginQr>,
    /// When the code arrived: it is uncovered from its top-left corner (`login-reveal`).
    code_at: Option<Instant>,
    scanned_at: Option<Instant>,
    transient: u32,
    done_at: Option<Instant>,
    clock: Instant,
    hover: Option<&'static str>,
    /// The platforms in the column's order, read when the window opens.
    rows: Vec<Platform>,
    query: String,
    /// Which row the highlight block stands on, sliding between rows (`st-t-move`).
    slide: Tween,
    slide_shown: bool,
    /// The sign-out question is open over the window.
    asking: bool,
    /// A failure's details are open.
    details: bool,
}

/// What the window draws with.
pub struct Look<'a> {
    pub i18n: &'a I18n,
    pub tokens: Tokens,
    pub daylight: bool,
    pub window: Size,
    pub platforms: Vec<Platform>,
    pub account: &'a AccountState,
    pub avatar: Option<&'a iced::widget::image::Handle>,
    /// The player holds songs: signing out empties it.
    pub has_track: bool,
}

impl Login {
    pub fn new(now: Instant) -> Self {
        Self {
            shown: Tween::new(0.0, UNFOLD_MS, EASE_OUT, now),
            open: false,
            chosen: None,
            round: 0,
            phase: Phase::Idle,
            qr: None,
            code_at: None,
            scanned_at: None,
            transient: 0,
            done_at: None,
            clock: now,
            hover: None,
            rows: Vec::new(),
            query: String::new(),
            slide: Tween::new(0.0, MOVE_MS, EASE_OUT, now),
            slide_shown: false,
            asking: false,
            details: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// `openLoginWindow`: opens on the platform in use when someone is signed in; a guest picks the platform.
    pub fn open(&mut self, provider: Option<&'static str>, signed_in: bool, omni: Option<&Omni>) -> Task<Message> {
        let now = Instant::now();
        self.open = true;
        self.shown.set_with(1.0, UNFOLD_MS, EASE_OUT, now);
        self.chosen = None;
        self.phase = Phase::Idle;
        self.qr = None;
        self.done_at = None;
        self.round += 1;
        self.rows = omni.map(Omni::platforms).unwrap_or_default();
        self.query.clear();
        self.hover = None;
        self.slide_shown = false;
        match provider {
            Some(provider) => self.choose(provider, signed_in, omni),
            None => Task::none(),
        }
    }

    /// Ends this round: the window fades out and a sign-in in progress stops.
    pub fn close(&mut self) {
        self.asking = false;
        self.open = false;
        self.round += 1;
        self.shown.set_with(0.0, MOVE_MS, EASE_OUT, Instant::now());
    }

    pub fn animating(&self, now: Instant) -> bool {
        if self.shown.running(now) || self.done_at.is_some() || self.slide.running(now) {
            return true;
        }
        let counting = matches!(&self.phase, Phase::Failed(failure) if now < failure.retry_opens());
        self.open
            && (counting || matches!(self.phase, Phase::Preparing | Phase::Code | Phase::Reading | Phase::Scanned))
    }

    /// Follows the account after a confirmed code: read means done, nobody means the account was not read.
    pub fn sync(&mut self, account: &AccountState) {
        if self.phase != Phase::Reading || self.done_at.is_some() {
            return;
        }
        match account {
            AccountState::SignedIn(_) => self.done_at = Some(Instant::now()),
            AccountState::Guest { .. } => self.fail(Failure::of(Case::AccountNotRead, Step::Account)),
            AccountState::Resolving => {}
        }
    }

    pub fn update(
        &mut self,
        message: Message,
        omni: Option<&Omni>,
        signed_in: bool,
    ) -> (Task<Message>, Option<Effect>) {
        match message {
            Message::Choose(provider) => {
                if self.chosen == Some(provider) && !matches!(self.phase, Phase::Failed(_)) {
                    return (Task::none(), None);
                }
                (self.choose(provider, signed_in, omni), None)
            }
            Message::Hover(provider) => {
                self.hover = provider;
                self.sync_slide();
                (Task::none(), None)
            }
            Message::Query(query) => {
                self.query = query;
                self.hover = None;
                self.sync_slide();
                (Task::none(), None)
            }
            Message::Close if self.asking => {
                self.asking = false;
                (Task::none(), None)
            }
            Message::Close => {
                self.close();
                (Task::none(), None)
            }
            Message::Details => {
                self.details = !self.details;
                (Task::none(), None)
            }
            Message::Retry => {
                let waiting = matches!(&self.phase, Phase::Failed(failure) if Instant::now() < failure.retry_opens());
                if signed_in || waiting {
                    return (Task::none(), None);
                }
                (self.start(omni), None)
            }
            Message::SignOut => {
                self.asking = true;
                (Task::none(), None)
            }
            Message::CancelSignOut => {
                self.asking = false;
                (Task::none(), None)
            }
            Message::ConfirmSignOut => {
                self.close();
                (Task::none(), Some(Effect::SignOut))
            }
            Message::Qr(round, result) if round == self.round => match result {
                Ok(qr) => {
                    self.qr = Some(qr);
                    self.code_at = Some(Instant::now());
                    self.phase = Phase::Code;
                    (self.check(omni), None)
                }
                Err(failure) => {
                    tracing::warn!(case = ?failure.case, "sign-in code unavailable");
                    self.fail(failure);
                    (Task::none(), None)
                }
            },
            Message::Poll(round) if round == self.round => (self.check(omni), None),
            Message::Checked(round, result) if round == self.round => self.checked(result),
            Message::Qr(..) | Message::Poll(_) | Message::Checked(..) | Message::Swallow => (Task::none(), None),
            Message::Frame => {
                let now = Instant::now();
                if let Some(at) = self.done_at {
                    let elapsed = now.saturating_duration_since(at).as_secs_f64() * 1000.0;
                    if elapsed >= DONE_END {
                        // Leaving `Reading` stops `sync` from starting the finished sign-in over again.
                        self.phase = Phase::Idle;
                        self.done_at = None;
                        self.open = false;
                        self.round += 1;
                        self.shown = Tween::new(0.0, MOVE_MS, EASE_OUT, now);
                    } else if elapsed >= DONE_LEAVE && self.open {
                        self.open = false;
                    }
                }
                (Task::none(), None)
            }
        }
    }

    fn choose(&mut self, provider: &'static str, signed_in: bool, omni: Option<&Omni>) -> Task<Message> {
        self.chosen = Some(provider);
        self.sync_slide();
        self.qr = None;
        self.round += 1;
        if signed_in {
            self.phase = Phase::Idle;
            return Task::none();
        }
        self.start(omni)
    }

    /// The rows the column shows: all of them, or those whose names hold the search words once there are twelve.
    fn shown_rows(&self) -> Vec<Platform> {
        let wanted = self.query.trim().to_lowercase();
        if self.rows.len() < SEARCH_FROM || wanted.is_empty() {
            return self.rows.clone();
        }
        let matches =
            |row: &&Platform| format!("{}\n{}", row.short_name, row.display_name).to_lowercase().contains(&wanted);
        self.rows.iter().filter(matches).copied().collect()
    }

    /// The highlighted row: the one pointed at, else the chosen one (`lit`).
    fn lit(&self, rows: &[Platform]) -> Option<usize> {
        let at = |id: Option<&'static str>| id.and_then(|id| rows.iter().position(|row| row.id == id));
        at(self.hover).or_else(|| at(self.chosen))
    }

    /// Slides the highlight to the lit row; the first one it lands on it simply appears on.
    fn sync_slide(&mut self) {
        let rows = self.shown_rows();
        let Some(index) = self.lit(&rows) else { return };
        let now = Instant::now();
        if self.slide_shown {
            self.slide.set(index as f32, now);
        } else {
            self.slide = Tween::new(index as f32, MOVE_MS, EASE_OUT, now);
            self.slide_shown = true;
        }
    }

    /// Asks for a new code (`startLogin` + `createQr`).
    fn start(&mut self, omni: Option<&Omni>) -> Task<Message> {
        self.round += 1;
        self.phase = Phase::Preparing;
        self.qr = None;
        self.scanned_at = None;
        self.transient = 0;
        let Some(omni) = omni.cloned() else {
            self.fail(Failure::of(Case::Unknown, Step::Start));
            return Task::none();
        };
        let round = self.round;
        let work = async move { omni.login_qr(PROVIDER).await.map_err(|err| Failure::from_error(&err, Step::Start)) };
        Task::perform(work, move |result| Message::Qr(round, result))
    }

    /// A new failure closes the details (`sceneKey` changed).
    fn fail(&mut self, failure: Failure) {
        self.details = false;
        self.phase = Phase::Failed(failure);
    }

    fn check(&self, omni: Option<&Omni>) -> Task<Message> {
        let (Some(omni), Some(qr)) = (omni.cloned(), self.qr.as_ref()) else { return Task::none() };
        let (round, key) = (self.round, qr.key.clone());
        let work = async move {
            omni.check_login(PROVIDER, &key)
                .await
                .map_err(|err| (Failure::from_error(&err, Step::Check), Failure::network(&err)))
        };
        Task::perform(work, move |result| Message::Checked(round, result))
    }

    /// One poll settled: the next is asked 2s later, never overlapping (`providerLoginSession`).
    fn checked(&mut self, result: Result<LoginState, (Failure, bool)>) -> (Task<Message>, Option<Effect>) {
        let state = match result {
            Ok(state) => state,
            Err((_, true)) if self.transient < TRANSIENT_LIMIT => {
                self.transient += 1;
                return (self.next_poll(), None);
            }
            Err((failure, _)) => {
                self.fail(failure);
                return (Task::none(), None);
            }
        };
        self.transient = 0;
        match state {
            LoginState::Waiting => {
                self.scanned_at = None;
                self.phase = Phase::Code;
                (self.next_poll(), None)
            }
            LoginState::Scanned => {
                let at = *self.scanned_at.get_or_insert_with(Instant::now);
                if at.elapsed() >= SCANNED_LIMIT {
                    self.fail(Failure::of(Case::ExpiredAfterScan, Step::Wait));
                    return (Task::none(), None);
                }
                self.phase = Phase::Scanned;
                (self.next_poll(), None)
            }
            LoginState::Confirmed => {
                self.phase = Phase::Reading;
                (Task::none(), Some(Effect::Confirmed))
            }
            LoginState::Expired => {
                let case = if self.scanned_at.is_some() { Case::ExpiredAfterScan } else { Case::CodeExpired };
                self.fail(Failure::of(case, Step::Wait));
                (Task::none(), None)
            }
            LoginState::Failed { code, message } => {
                let raw = [Some(format!("code {code}")), message.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                let quote = message.map_or_else(|| (code.to_string(), None), |text| (text, Some(code.to_string())));
                self.fail(Failure { quote: Some(quote), raw, ..Failure::of(Case::Refused, Step::Check) });
                (Task::none(), None)
            }
        }
    }

    fn next_poll(&self) -> Task<Message> {
        let round = self.round;
        Task::perform(blocking(|| std::thread::sleep(POLL)), move |_| Message::Poll(round))
    }

    pub fn view<'a>(&'a self, look: Look<'a>, now: Instant) -> Option<Element<'a, Message>> {
        let shown = self.shown.value(now);
        let done = self.done_at.map(|at| now.saturating_duration_since(at).as_secs_f64() * 1000.0);
        let leaving = done.map_or(0.0, |ms| part(ms, DONE_LEAVE, DONE_LEAVE + UNFOLD_MS, EASE_STANDARD));
        let opacity = shown * (1.0 - leaving);
        if opacity <= 0.002 {
            return None;
        }
        let tokens = look.tokens.faded(opacity);
        let scrim_color =
            if look.daylight { Color::from_rgba8(24, 18, 12, 0.36) } else { Color { a: 0.52, ..Color::BLACK } };
        let scrim =
            mouse_area(container(space()).width(Length::Fill).height(Length::Fill).style(move |_theme: &Theme| {
                container::Style {
                    background: Some(Background::Color(Color { a: scrim_color.a * opacity, ..scrim_color })),
                    ..container::Style::default()
                }
            }))
            .on_press(Message::Close);
        let scrim = iced::widget::opaque(scrim);

        let narrow = look.window.width < WIDTH + 2.0 * MARGIN;
        let width = WIDTH.min(look.window.width - 2.0 * MARGIN).max(1.0);
        let platform = self.chosen.and_then(|id| look.platforms.iter().find(|platform| platform.id == id)).copied();
        let pc = platform.map(|platform| folia_visualizer::color(Rgb::hex(platform.color), 1.0));
        let signed_in = matches!(look.account, AccountState::SignedIn(_));
        let content = self.content(&look, tokens, narrow, platform, signed_in, now);
        let mut sheet_layers = stack![];
        if let (Some(pc), false) = (pc, narrow) {
            let rings = matches!(self.phase, Phase::Code) && done.is_none();
            sheet_layers = sheet_layers.push(
                canvas_widget(Underlay { pc, rings, clock: self.clock, opacity })
                    .width(Length::Fixed(width - 2.0))
                    .height(Length::Fixed(self.rest_height(signed_in))),
            );
        }
        sheet_layers = sheet_layers.push(content);
        if let (Some(ms), Some(platform)) = (done, platform) {
            sheet_layers = sheet_layers.push(flood(&look, platform, ms, opacity, narrow));
        }
        let shadow = Color { a: if look.daylight { 0.26 } else { 0.6 } * opacity, ..Color::BLACK };
        let sheet = container(sheet_layers).width(Length::Fixed(width - 2.0)).height(Length::Shrink).clip(true).style(
            move |_theme: &Theme| container::Style {
                background: Some(Background::Color(tokens.win)),
                border: Border { radius: 16.0.into(), ..Border::default() },
                ..container::Style::default()
            },
        );
        // `0 0 0 1px var(--st-line2)` is a ring outside the window: drawn as a 1px wider plate behind it.
        let line2 = tokens.line2;
        let sheet = container(sheet).padding(1).style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(line2)),
            border: Border { radius: 17.0.into(), ..Border::default() },
            shadow: Shadow { color: shadow, offset: Vector::new(0.0, 36.0), blur_radius: 90.0 },
            ..container::Style::default()
        });
        let scale = if leaving > 0.0 { 1.0 + 0.08 * leaving } else { 0.9 + 0.1 * shown };
        let sheet = mouse_area(sheet).on_press(Message::Swallow);
        // Scaling lifts a subtree to the top layer in iced, so it is only scaled while it moves.
        let sheet: Element<'a, Message> =
            if (scale - 1.0).abs() > 0.0005 { iced::widget::float(sheet).scale(scale).into() } else { sheet.into() };
        let top = MARGIN.max(look.window.height / 2.0 - REST_HEIGHT / 2.0);
        let placed = container(sheet)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Start)
            .padding(Padding { top, right: MARGIN, bottom: MARGIN, left: MARGIN });
        let mut layers = stack![scrim, placed].width(Length::Fill).height(Length::Fill);
        if self.asking {
            layers = layers.push(self.question(&look, tokens, platform, look.has_track));
        }
        Some(layers.into())
    }

    /// `ConfirmDialog` in a 400px sheet: the title, the nickname (and that the queue empties), cancel and a red sign-out.
    fn question<'a>(
        &'a self,
        look: &Look<'a>,
        tokens: Tokens,
        platform: Option<Platform>,
        empties: bool,
    ) -> Element<'a, Message> {
        let i18n = look.i18n;
        let label = platform.map(|platform| platform.short_name).unwrap_or_default();
        let title = i18n.format("account.logoutConfirmTitle", &[("provider", label)]);
        let mut lines = Vec::new();
        if let AccountState::SignedIn(account) = look.account {
            lines.push(account.nickname.clone());
        }
        if empties {
            lines.push(i18n.t("account.logoutClearsQueue").to_owned());
        }
        let close = button(container(icon(Icon::X, 16.0, tokens.t2)).center(Length::Fill))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .padding(0)
            .on_press(Message::CancelSignOut)
            .style(button::text);
        let header = row![
            container(text(title).size(16).font(SEMIBOLD).color(tokens.t1))
                .width(Length::Fill)
                .height(Length::Fixed(32.0))
                .align_y(Alignment::Center),
            close
        ]
        .spacing(12)
        .padding(Padding { top: 16.0, right: 16.0, bottom: 0.0, left: 24.0 });
        let mut body = column![header];
        if !lines.is_empty() {
            body = body.push(
                container(
                    text(lines.join("\n"))
                        .size(13)
                        .line_height(iced::widget::text::LineHeight::Absolute(21.0.into()))
                        .color(tokens.t2),
                )
                .padding(Padding { top: 4.0, right: 24.0, bottom: 16.0, left: 24.0 }),
            );
        }
        let err = if look.daylight { Color::from_rgb8(0xd0, 0x44, 0x44) } else { Color::from_rgb8(0xef, 0x6f, 0x6f) };
        let on_err = if look.daylight { Color::WHITE } else { Color::from_rgb8(0x1b, 0x10, 0x12) };
        let danger =
            Tokens { accent: Color { a: tokens.t1.a, ..err }, on_accent: Color { a: tokens.t1.a, ..on_err }, ..tokens };
        let footer = row![
            space().width(Length::Fill),
            kit_button(i18n.t("ui.cancel").to_owned(), false, Message::CancelSignOut, tokens),
            kit_button(i18n.t("account.logout").to_owned(), true, Message::ConfirmSignOut, danger),
        ]
        .spacing(8)
        .padding(Padding { top: 4.0, right: 16.0, bottom: 16.0, left: 16.0 });
        let shadow = Color { a: if look.daylight { 0.26 } else { 0.6 } * tokens.t1.a, ..Color::BLACK };
        let line2 = tokens.line2;
        let sheet = container(body.push(footer))
            .width(Length::Fixed(398.0_f32.min(look.window.width - 2.0 * MARGIN - 2.0)))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(tokens.win)),
                border: Border { radius: 16.0.into(), ..Border::default() },
                ..container::Style::default()
            });
        let sheet = container(sheet).padding(1).style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(line2)),
            border: Border { radius: 17.0.into(), ..Border::default() },
            shadow: Shadow { color: shadow, offset: Vector::new(0.0, 36.0), blur_radius: 90.0 },
            ..container::Style::default()
        });
        let scrim_color =
            if look.daylight { Color::from_rgba8(24, 18, 12, 0.36) } else { Color { a: 0.52, ..Color::BLACK } };
        let scrim =
            mouse_area(container(space()).width(Length::Fill).height(Length::Fill).style(move |_theme: &Theme| {
                container::Style { background: Some(Background::Color(scrim_color)), ..container::Style::default() }
            }))
            .on_press(Message::CancelSignOut);
        stack![
            iced::widget::opaque(scrim),
            container(mouse_area(sheet).on_press(Message::Swallow)).center(Length::Fill)
        ]
        .into()
    }

    fn content<'a>(
        &'a self,
        look: &Look<'a>,
        tokens: Tokens,
        narrow: bool,
        platform: Option<Platform>,
        signed_in: bool,
        now: Instant,
    ) -> Element<'a, Message> {
        let i18n = look.i18n;
        let label = platform.map(|platform| platform.short_name).unwrap_or_default();
        let title = if platform.is_some() {
            i18n.format("home.loginToProvider", &[("provider", label)])
        } else {
            i18n.t("login.title").to_owned()
        };
        let mut heading = row![].spacing(10).align_y(Alignment::Center);
        if let Some(platform) = platform {
            heading = heading.push(mark_faded(platform, 24.0, tokens.t1.a));
        }
        heading = heading
            .push(text(title).size(16).font(SEMIBOLD).color(tokens.t1).wrapping(iced::widget::text::Wrapping::None));
        let close = button(container(icon(Icon::X, 16.0, tokens.t2)).center(Length::Fill))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .padding(0)
            .on_press(Message::Close)
            .style(move |_theme: &Theme, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(tokens.fill)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..button::Style::default()
            });
        let close = iced::widget::Tooltip::new(
            close,
            text(i18n.t("home.closeLogin")).size(12),
            iced::widget::tooltip::Position::Bottom,
        );
        let header =
            row![container(heading).width(Length::Fill).height(Length::Fixed(32.0)).align_y(Alignment::Center), close]
                .spacing(12)
                .padding(Padding { top: 16.0, right: 16.0, bottom: 0.0, left: 24.0 });

        let tile = self.tile(look, tokens, platform, signed_in, now);
        let message = self.message(look, tokens, platform, signed_in, now);
        let main: Element<'a, Message> = if narrow {
            column![
                header,
                container(self.platforms(look, tokens, true, now)).padding([0.0, 16.0]),
                container(tile).center_x(Length::Fill),
                container(message).padding(Padding { top: 0.0, right: 24.0, bottom: 20.0, left: 24.0 }),
            ]
            .spacing(16)
            .into()
        } else {
            let mut body = column![row![tile, message].spacing(24)].padding(Padding {
                top: 82.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            });
            if let (Phase::Failed(failure), true) = (&self.phase, self.details_shown(signed_in)) {
                body = body.push(details(look, tokens, label, failure));
            }
            column![header, body].into()
        };
        if narrow {
            return main;
        }
        let aside = container(self.platforms(look, tokens, false, now))
            .width(Length::Fixed(ASIDE))
            .height(Length::Fill)
            .padding(8)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(tokens.side)),
                border: Border {
                    radius: iced::border::Radius::default().top_left(16.0).bottom_left(16.0),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        let rule = container(space()).width(Length::Fixed(1.0)).height(Length::Fill).style(move |_theme: &Theme| {
            container::Style { background: Some(Background::Color(tokens.line)), ..container::Style::default() }
        });
        row![aside, rule, container(main).width(Length::Fill)].height(Length::Fixed(self.rest_height(signed_in))).into()
    }

    /// `PlatformColumn`: one row a platform in the column's order; a block slides to the row pointed at or chosen,
    /// the chosen row has a 3px bar of its colour at its left edge, and unconfigured rows are grey.
    fn platforms<'a>(&'a self, look: &Look<'a>, tokens: Tokens, lie: bool, now: Instant) -> Element<'a, Message> {
        let i18n = look.i18n;
        let rows = self.shown_rows();
        let searching = self.rows.len() >= SEARCH_FROM;
        // `ROW_SIZE`: rows share the column's height, each 46 to 68 high.
        let room = REST_HEIGHT - 16.0 - if searching { 38.0 } else { 0.0 };
        let height = if lie { 36.0 } else { (room / rows.len().max(1) as f32).clamp(46.0, 68.0) };
        let lit = self.lit(&rows);
        let lit_id = lit.and_then(|index| rows.get(index)).map(|row| row.id);
        let items = rows.iter().map(|&platform| {
            let bright = self.chosen == Some(platform.id) || lit_id == Some(platform.id);
            self.platform_row(look, tokens, platform, lie, height, bright)
        });
        let list: Element<'a, Message> = if lie {
            iced::widget::scrollable(row(items).spacing(4))
                .direction(iced::widget::scrollable::Direction::Horizontal(
                    iced::widget::scrollable::Scrollbar::new().width(0).scroller_width(0),
                ))
                .into()
        } else if rows.is_empty() {
            container(text(i18n.t("login.noPlatformMatch")).size(13).color(tokens.t3))
                .padding([24.0, 0.0])
                .center_x(Length::Fill)
                .into()
        } else {
            let fill2 = tokens.fill2;
            let block: Element<'a, Message> = match lit {
                Some(_) => column![
                    space().height(Length::Fixed(self.slide.value(now) * height)),
                    container(space()).width(Length::Fill).height(Length::Fixed(height)).style(
                        move |_theme: &Theme| {
                            container::Style {
                                background: Some(Background::Color(fill2)),
                                border: Border { radius: 8.0.into(), ..Border::default() },
                                ..container::Style::default()
                            }
                        }
                    ),
                ]
                .into(),
                None => space().into(),
            };
            let total = height * rows.len() as f32;
            stack![container(block).width(Length::Fill).height(Length::Fixed(total)), column(items).width(Length::Fill)]
                .into()
        };
        if !searching {
            return list;
        }
        let field = iced::widget::text_input(i18n.t("login.searchPlatforms"), &self.query)
            .on_input(Message::Query)
            .size(13)
            .padding(0)
            .style(move |_theme: &Theme, _status| iced::widget::text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: tokens.t3,
                placeholder: tokens.t3,
                value: tokens.t1,
                selection: tokens.fill2,
            });
        let fill = tokens.fill;
        let search = container(row![icon(Icon::Search, 14.0, tokens.t3), field].spacing(8).align_y(Alignment::Center))
            .height(Length::Fixed(32.0))
            .padding([0.0, 10.0])
            .align_y(Alignment::Center)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(fill)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..container::Style::default()
            });
        column![search, list].spacing(6).into()
    }

    fn platform_row<'a>(
        &'a self,
        look: &Look<'a>,
        tokens: Tokens,
        platform: Platform,
        lie: bool,
        height: f32,
        bright: bool,
    ) -> Element<'a, Message> {
        let i18n = look.i18n;
        let chosen = self.chosen == Some(platform.id);
        let off = !platform.available;
        let signed_in = platform.id == PROVIDER && matches!(look.account, AccountState::SignedIn(_));
        let ran_out = platform.id == PROVIDER && matches!(look.account, AccountState::Guest { ran_out: true });
        let dim = if off { 0.42 } else { 1.0 };
        let fade = |color: Color| Color { a: color.a * dim, ..color };
        let color = fade(if bright { tokens.t1 } else { tokens.t2 });
        let mut face = row![].spacing(if lie { 10 } else { 12 }).align_y(Alignment::Center);
        face = face.push(mark_faded(platform, if lie { 22.0 } else { 30.0 }, dim * tokens.t1.a));
        if lie {
            face = face.push(
                container(text(platform.short_name).size(13).color(color).wrapping(iced::widget::text::Wrapping::None))
                    .max_width(120.0),
            );
        } else {
            let note: Element<'a, Message> = if signed_in {
                let name = match look.account {
                    AccountState::SignedIn(account) => account.nickname.clone(),
                    _ => i18n.t("login.rowSignedIn").to_owned(),
                };
                let ok =
                    if look.daylight { Color::from_rgb8(0x1e, 0x9a, 0x5c) } else { Color::from_rgb8(0x56, 0xc9, 0x8d) };
                row![
                    dot(Color { a: tokens.t1.a, ..ok }, 6.0),
                    text(name).size(11.5).color(tokens.t3).wrapping(iced::widget::text::Wrapping::None)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            } else {
                let (key, note) = if ran_out {
                    ("login.rowRanOut", warn(look.daylight))
                } else if off {
                    ("home.providerNotConfigured", fade(tokens.t3))
                } else {
                    ("login.rowSignedOut", tokens.t3)
                };
                text(i18n.t(key)).size(11.5).color(note).wrapping(iced::widget::text::Wrapping::None).into()
            };
            let names = column![
                text(platform.short_name)
                    .size(13)
                    .font(MEDIUM)
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None),
                note
            ]
            .spacing(2);
            face = face.push(container(names).width(Length::Fill).clip(true));
        }
        if let (true, Some(avatar)) = (signed_in, look.avatar) {
            let size = if lie { 18.0 } else { 24.0 };
            face = face.push(
                iced::widget::image(avatar.clone())
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .border_radius(size / 2.0)
                    .opacity(tokens.t1.a),
            );
        }
        let fill2 = tokens.fill2;
        let row_button = button(container(face).height(Length::Fill).align_y(Alignment::Center))
            .width(if lie { Length::Shrink } else { Length::Fill })
            .height(Length::Fixed(height))
            .padding(if lie { Padding::from([0.0, 8.0]) } else { Padding::from([0.0, 10.0]) })
            .on_press_maybe((!off).then_some(Message::Choose(platform.id)))
            .style(move |_theme: &Theme, _status| button::Style {
                background: (lie && bright).then_some(Background::Color(fill2)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                ..button::Style::default()
            });
        let item: Element<'a, Message> = if lie {
            row_button.into()
        } else {
            // The chosen row's bar sits on the row's left edge, over its padding.
            let pc = folia_visualizer::color(Rgb::hex(platform.color), if chosen { tokens.t1.a } else { 0.0 });
            let bar = container(space()).width(Length::Fixed(3.0)).height(Length::Fixed(height * 0.38)).style(
                move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(pc)),
                    border: Border { radius: 2.0.into(), ..Border::default() },
                    ..container::Style::default()
                },
            );
            stack![row_button, container(bar).height(Length::Fixed(height)).align_y(Alignment::Center)].into()
        };
        let id = platform.id;
        let hovered: Element<'a, Message> = mouse_area(item)
            .on_enter(if off { Message::Swallow } else { Message::Hover(Some(id)) })
            .on_exit(Message::Hover(None))
            .into();
        if off {
            iced::widget::Tooltip::new(
                hovered,
                text(i18n.t("home.providerNotConfigured")).size(12),
                iced::widget::tooltip::Position::Right,
            )
            .into()
        } else {
            hovered
        }
    }

    /// `LoginTile`: 200px square, a picture for each scene.
    fn tile<'a>(
        &'a self,
        look: &Look<'a>,
        tokens: Tokens,
        platform: Option<Platform>,
        signed_in: bool,
        now: Instant,
    ) -> Element<'a, Message> {
        let card = if look.daylight { Color::WHITE } else { Color::from_rgb8(0x23, 0x23, 0x27) };
        let card = Color { a: tokens.win.a, ..card };
        let pc = platform.map_or(tokens.t3, |platform| folia_visualizer::color(Rgb::hex(platform.color), tokens.t1.a));
        let spin = now.saturating_duration_since(self.clock).as_secs_f32();
        let expired = matches!(&self.phase, Phase::Failed(failure) if failure.case.expired_code()) && self.qr.is_some();
        let on_white = (self.phase == Phase::Code && self.qr.is_some()) || expired;
        let framed = platform.is_none();
        let disc = |tone: Tone, glyph: Icon, arc: bool| -> Element<'a, Message> {
            let (fill, ink) = tone_colors(tone, tokens, look.daylight);
            let base = container(icon(glyph, 34.0, ink)).center(Length::Fixed(88.0)).style(move |_theme: &Theme| {
                container::Style {
                    background: Some(Background::Color(fill)),
                    border: Border { radius: 44.0.into(), ..Border::default() },
                    ..container::Style::default()
                }
            });
            if arc {
                stack![
                    base,
                    canvas_widget(Arc { line: tokens.line, pc, turn: spin })
                        .width(Length::Fixed(88.0))
                        .height(Length::Fixed(88.0))
                ]
                .into()
            } else {
                base.into()
            }
        };
        let picture: Element<'a, Message> = match (&self.phase, platform) {
            (_, None) => {
                let (fill, _) = tone_colors(Tone::Calm, tokens, look.daylight);
                container(icon(Icon::User, 38.0, Color { a: tokens.t2.a * 0.62 / 0.62, ..tokens.t1 }))
                    .center(Length::Fixed(88.0))
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(Background::Color(fill)),
                        border: Border { radius: 44.0.into(), ..Border::default() },
                        ..container::Style::default()
                    })
                    .into()
            }
            (_, Some(platform)) if signed_in => account_picture(look, platform, 88.0, tokens),
            (Phase::Code, Some(_)) => match &self.qr {
                Some(qr) => canvas_widget(Code { qr, dim: 1.0, opacity: tokens.t1.a, reveal: self.reveal(now) })
                    .width(Length::Fixed(176.0))
                    .height(Length::Fixed(176.0))
                    .into(),
                None => space().into(),
            },
            (Phase::Failed(_), Some(_)) if expired => {
                let qr = self.qr.as_ref();
                let code: Element<'a, Message> = match qr {
                    Some(qr) => canvas_widget(Code { qr, dim: 0.1, opacity: tokens.t1.a, reveal: 1.0 })
                        .width(Length::Fixed(176.0))
                        .height(Length::Fixed(176.0))
                        .into(),
                    None => space().into(),
                };
                let alpha = tokens.t1.a;
                let knob = container(icon(Icon::RefreshCw, 22.0, Color { a: alpha, ..Color::WHITE }))
                    .center(Length::Fixed(56.0))
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(Background::Color(Color { a: alpha, ..Color::from_rgb8(0x17, 0x17, 0x1a) })),
                        border: Border { radius: 28.0.into(), ..Border::default() },
                        ..container::Style::default()
                    });
                let refresh =
                    button(stack![container(code).center(Length::Fill), container(knob).center(Length::Fill)])
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(0)
                        .on_press(Message::Retry)
                        .style(button::text);
                refresh.into()
            }
            (Phase::Failed(failure), Some(_)) => {
                let (glyph, tone) = failure.case.glyph();
                disc(tone, glyph, false)
            }
            (Phase::Scanned, Some(_)) => disc(Tone::Calm, Icon::Smartphone, false),
            (Phase::Reading, Some(_)) => disc(Tone::Calm, Icon::User, true),
            (Phase::Preparing | Phase::Idle, Some(platform)) => stack![
                container(mark_faded(platform, 30.0, tokens.t1.a)).center(Length::Fixed(54.0)),
                canvas_widget(Arc { line: tokens.line, pc, turn: spin })
                    .width(Length::Fixed(54.0))
                    .height(Length::Fixed(54.0)),
            ]
            .into(),
        };
        let background = if on_white {
            Color { a: tokens.t1.a, ..Color::WHITE }
        } else if framed {
            Color::TRANSPARENT
        } else {
            card
        };
        let ring = if framed {
            tokens.line2
        } else if on_white {
            Color::TRANSPARENT
        } else {
            tokens.line
        };
        container(picture)
            .center(Length::Fixed(TILE))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(background)),
                border: Border { radius: 12.0.into(), width: 1.0, color: ring },
                ..container::Style::default()
            })
            .into()
    }

    /// `LoginMessage`: the headline, the line under it, the platform's own words, the actions and the step marks.
    fn message<'a>(
        &'a self,
        look: &Look<'a>,
        tokens: Tokens,
        platform: Option<Platform>,
        signed_in: bool,
        now: Instant,
    ) -> Element<'a, Message> {
        let i18n = look.i18n;
        let label = platform.map(|platform| platform.short_name).unwrap_or_default();
        let mut note: Option<String> = None;
        let mut quote: Option<(String, Option<String>)> = None;
        let mut actions: Vec<(String, bool, Message)> = Vec::new();
        let headline = match (&self.phase, platform) {
            (_, None) => i18n.t("login.choosePlatform").to_owned(),
            (_, Some(_)) if signed_in => {
                let AccountState::SignedIn(account) = look.account else { return space().into() };
                note = Some(format!("{label} · ID: {}", account.id));
                actions.push((i18n.t("account.logout").to_owned(), false, Message::SignOut));
                account_name(account, label)
            }
            (Phase::Failed(failure), Some(_)) => {
                let (title, line) = failure.case.texts();
                note = line.map(|key| i18n.t(key).to_owned());
                quote = failure.quote.clone();
                if now >= failure.retry_opens() {
                    actions.push((i18n.t("home.retryQr").to_owned(), true, Message::Retry));
                }
                i18n.format(title, &[("provider", label)])
            }
            (Phase::Scanned, Some(_)) => i18n.t("home.qrScanned").to_owned(),
            (Phase::Reading, Some(_)) => i18n.t("login.readingAccount").to_owned(),
            (Phase::Code | Phase::Preparing | Phase::Idle, Some(_)) => {
                note = Some(i18n.t("home.loginNote").to_owned());
                i18n.t("home.loginTitle").to_owned()
            }
        };
        let mut words = column![
            text(headline)
                .size(20)
                .font(SEMIBOLD)
                .line_height(iced::widget::text::LineHeight::Absolute(28.0.into()))
                .color(tokens.t1)
        ];
        if let Some(note) = note {
            words = words.push(
                container(
                    text(note)
                        .size(13)
                        .line_height(iced::widget::text::LineHeight::Absolute(20.0.into()))
                        .color(tokens.t2),
                )
                .padding(Padding::ZERO.top(6.0)),
            );
        }
        if let Some((said, code)) = quote {
            let mut line = row![
                text(label.to_owned()).size(12).font(SEMIBOLD).color(tokens.t1),
                container(text(said).size(12).color(tokens.t2)).width(Length::Fill)
            ]
            .spacing(8)
            .align_y(Alignment::Start);
            if let Some(code) = code {
                line = line.push(text(code).size(12).font(Font::MONOSPACE).color(tokens.t3));
            }
            let fill = tokens.fill;
            words = words.push(
                container(container(line).padding([8.0, 10.0]).width(Length::Fill).style(move |_theme: &Theme| {
                    container::Style {
                        background: Some(Background::Color(fill)),
                        border: Border { radius: 8.0.into(), ..Border::default() },
                        ..container::Style::default()
                    }
                }))
                .padding(Padding::ZERO.top(12.0)),
            );
        }
        let mut buttons = row![].spacing(8).align_y(Alignment::Center);
        let failure = match &self.phase {
            Phase::Failed(failure) if platform.is_some() && !signed_in => Some(failure),
            _ => None,
        };
        if let Some(failure) = failure.filter(|failure| now < failure.retry_opens()) {
            buttons = buttons.push(wait_button(i18n, failure, tokens, now));
        }
        for (label, primary, message) in actions {
            buttons = buttons.push(kit_button(label, primary, message, tokens));
        }
        if failure.is_some_and(|failure| failure.case.details()) {
            buttons = buttons.push(details_toggle(i18n, self.details, tokens));
        }
        buttons = buttons.push(space().width(Length::Fill));
        if let (Some(platform), Some(current)) = (platform, self.step()) {
            buttons = buttons.push(steps(current, platform, tokens));
        }
        column![container(words).height(Length::Fill), buttons.height(Length::Fixed(32.0))]
            .spacing(16)
            .height(Length::Fixed(TILE))
            .width(Length::Fill)
            .into()
    }

    /// How far the code is uncovered: `KIT_MOTION.show` (480ms) with the kit's `out` curve.
    fn reveal(&self, now: Instant) -> f32 {
        self.code_at.map_or(1.0, |at| {
            let ms = now.saturating_duration_since(at).as_secs_f64() * 1000.0;
            part(ms, 0.0, 480.0, EASE_OUT)
        })
    }

    /// The window's height: the open details add their share under the tile and the message.
    fn rest_height(&self, signed_in: bool) -> f32 {
        if self.details_shown(signed_in) { REST_HEIGHT + DETAILS_HEIGHT } else { REST_HEIGHT }
    }

    fn details_shown(&self, signed_in: bool) -> bool {
        self.details
            && !signed_in
            && self.chosen.is_some()
            && matches!(&self.phase, Phase::Failed(failure) if failure.case.details())
    }

    /// `stepMarks` for a QR sign-in: code, phone, account.
    fn step(&self) -> Option<usize> {
        match self.phase {
            Phase::Preparing | Phase::Code => Some(0),
            Phase::Scanned => Some(1),
            Phase::Reading => Some(2),
            Phase::Idle | Phase::Failed(_) => None,
        }
    }
}

fn account_name(account: &Account, label: &str) -> String {
    if account.nickname.is_empty() { label.to_owned() } else { account.nickname.clone() }
}

/// `--st-warn`.
fn warn(daylight: bool) -> Color {
    if daylight { Color::from_rgb8(0xb9, 0x7a, 0x10) } else { Color::from_rgb8(0xe3, 0xa9, 0x49) }
}

/// `TONE_CLASS`: a fill and an ink for the failure discs.
fn tone_colors(tone: Tone, tokens: Tokens, daylight: bool) -> (Color, Color) {
    match tone {
        Tone::Calm => (tokens.fill, tokens.t2),
        Tone::Warn => {
            let ink = Color { a: tokens.t1.a, ..warn(daylight) };
            (Color { a: 0.14 * ink.a, ..ink }, ink)
        }
        Tone::Error => {
            let err = if daylight { Color::from_rgb8(0xd0, 0x44, 0x44) } else { Color::from_rgb8(0xef, 0x6f, 0x6f) };
            let ink = Color { a: tokens.t1.a, ..err };
            (Color { a: 0.13 * ink.a, ..ink }, ink)
        }
    }
}

fn dot<'a>(color: Color, size: f32) -> Element<'a, Message> {
    container(space())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: (size / 2.0).into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

/// `PlatformMark`: the platform's app icon, or its letter on its colour, 42% of the size, dark or white by contrast.
fn mark_faded<'a>(platform: Platform, size: f32, alpha: f32) -> Element<'a, Message> {
    if let Some(handle) = platform_icon(platform.id) {
        return iced::widget::image(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .border_radius(size / 2.0)
            .opacity(alpha)
            .into();
    }
    let color = folia_visualizer::color(Rgb::hex(platform.color), alpha);
    let on = Color { a: alpha, ..on_color(platform.color) };
    container(
        text(platform.letter.to_string())
            .size((size * 0.42).round())
            .font(crate::typeface::font(iced::font::Weight::Bold))
            .color(on),
    )
    .center(Length::Fixed(size))
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: (size / 2.0).into(), ..Border::default() },
        ..container::Style::default()
    })
    .into()
}

/// `onAccentColor`: `#17171a` or white, whichever contrasts more.
fn on_color(hex: u32) -> Color {
    let luminance = |hex: u32| {
        let channel = |shift: u32| {
            let value = ((hex >> shift) & 0xff) as f32 / 255.0;
            if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let contrast = |a: f32, b: f32| (a.max(b) + 0.05) / (a.min(b) + 0.05);
    let base = luminance(hex);
    if contrast(luminance(0x17171a), base) >= contrast(1.0, base) {
        Color::from_rgb8(0x17, 0x17, 0x1a)
    } else {
        Color::WHITE
    }
}

/// `AccountPicture`: the avatar or the nickname's first character, with the platform's mark in the corner.
fn account_picture<'a>(look: &Look<'a>, platform: Platform, size: f32, tokens: Tokens) -> Element<'a, Message> {
    let picture: Element<'a, Message> = match look.avatar {
        Some(handle) => iced::widget::image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(iced::ContentFit::Cover)
            .border_radius(size / 2.0)
            .opacity(tokens.t1.a)
            .into(),
        None => {
            let initial = match look.account {
                AccountState::SignedIn(account) => {
                    account.nickname.trim().chars().next().map(String::from).unwrap_or_default()
                }
                _ => String::new(),
            };
            let fill = tokens.fill2;
            container(text(initial).size((size * 0.42).round()).font(SEMIBOLD).color(tokens.t1))
                .center(Length::Fixed(size))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(fill)),
                    border: Border { radius: (size / 2.0).into(), ..Border::default() },
                    ..container::Style::default()
                })
                .into()
        }
    };
    let corner = (size * 0.44).round().max(14.0);
    let card = if look.daylight { Color::WHITE } else { Color::from_rgb8(0x23, 0x23, 0x27) };
    let card = Color { a: tokens.t1.a, ..card };
    let badge =
        container(mark_faded(platform, corner, tokens.t1.a)).padding(2).style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(card)),
            border: Border { radius: (corner / 2.0 + 2.0).into(), ..Border::default() },
            ..container::Style::default()
        });
    // The mark sits 3px past the picture's bottom-right edge, laid out in a box that much larger.
    let reach = size + 3.0;
    stack![
        container(picture).width(Length::Fixed(reach)).height(Length::Fixed(reach)),
        container(badge)
            .width(Length::Fixed(reach))
            .height(Length::Fixed(reach))
            .align_x(Alignment::End)
            .align_y(Alignment::End),
    ]
    .into()
}

/// The kit's `Button`: 32px high, 13px, a fill or the accent.
fn kit_button<'a>(label: String, primary: bool, message: Message, tokens: Tokens) -> Element<'a, Message> {
    let color = if primary { tokens.on_accent } else { tokens.t1 };
    button(container(text(label).size(13).color(color)).height(Length::Fill).align_y(Alignment::Center))
        .height(Length::Fixed(32.0))
        .padding([0.0, 12.0])
        .on_press(message)
        .style(move |_theme: &Theme, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let fill = match (primary, hot) {
                (true, false) => tokens.accent,
                (true, true) => Color { a: tokens.accent.a * 0.9, ..tokens.accent },
                (false, false) => tokens.fill,
                (false, true) => tokens.fill2,
            };
            button::Style {
                background: Some(Background::Color(fill)),
                border: Border { radius: 8.0.into(), ..Border::default() },
                text_color: color,
                ..button::Style::default()
            }
        })
        .into()
}

/// `WaitButton`: counts down to the retry, its fill growing a step each second.
fn wait_button<'a>(i18n: &I18n, failure: &Failure, tokens: Tokens, now: Instant) -> Element<'a, Message> {
    let ends = failure.retry_opens();
    let left = ends.saturating_duration_since(now).as_secs_f32();
    let total = ends.saturating_duration_since(failure.at).as_secs_f32().ceil().max(1.0);
    // `(total - left + 1) / total`, slid over each second as the width transition does.
    let grown = ((total - left + 1.0) / total).clamp(0.0, 1.0);
    let seconds = (left.ceil() as u32).to_string();
    let label = text(i18n.format("login.retryIn", &[("seconds", seconds.as_str())])).size(13).color(tokens.t2);
    let fill2 = tokens.fill2;
    let bar = row![
        container(space()).width(Length::FillPortion((grown * 1000.0) as u16)).height(Length::Fill).style(
            move |_theme: &Theme| container::Style {
                background: Some(Background::Color(fill2)),
                ..container::Style::default()
            }
        ),
        space().width(Length::FillPortion(((1.0 - grown) * 1000.0) as u16)),
    ];
    let fill = tokens.fill;
    container(stack![container(label).height(Length::Fixed(32.0)).padding([0.0, 12.0]).align_y(Alignment::Center), bar])
        .clip(true)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

/// The details toggle: `login.details` and a chevron that turns over when open.
fn details_toggle<'a>(i18n: &I18n, open: bool, tokens: Tokens) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronUp } else { Icon::ChevronDown };
    let face = row![text(i18n.t("login.details").to_owned()).size(13).color(tokens.t2), icon(chevron, 14.0, tokens.t2)]
        .spacing(4)
        .align_y(Alignment::Center);
    button(container(face).height(Length::Fill).align_y(Alignment::Center))
        .height(Length::Fixed(32.0))
        .padding([0.0, 8.0])
        .on_press(Message::Details)
        .style(move |_theme: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(Background::Color(tokens.fill)),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .into()
}

/// `LoginDetails`: the failure's facts, under a rule.
fn details<'a>(look: &Look<'a>, tokens: Tokens, label: &str, failure: &Failure) -> Element<'a, Message> {
    let i18n = look.i18n;
    let (id, kind) = failure.case.kind();
    let raw = if failure.raw.is_empty() { "—".to_owned() } else { failure.raw.clone() };
    let facts = [
        (i18n.t("login.factPlatform"), label.to_owned()),
        (i18n.t("login.factStep"), i18n.t(failure.step.text()).to_owned()),
        (i18n.t("login.factKind"), format!("{} · {id}", i18n.t(kind))),
        (i18n.t("login.factRaw"), raw),
    ];
    // `grid-cols-[auto_minmax(0,1fr)]`: the names' column is as wide as the widest name.
    let span = |name: &str| name.chars().map(|c| if c.is_ascii() { 6.6 } else { 12.0 }).sum::<f32>();
    let name_width = facts.iter().map(|(name, _)| span(name)).fold(0.0, f32::max).ceil();
    let leading = iced::widget::text::LineHeight::Absolute(20.0.into());
    let rows = column(facts.into_iter().map(|(name, value)| {
        row![
            text(name.to_owned()).size(12).line_height(leading).color(tokens.t3).width(Length::Fixed(name_width)),
            text(value).size(12).line_height(leading).font(Font::MONOSPACE).color(tokens.t2).width(Length::Fill),
        ]
        .spacing(16)
        .into()
    }))
    .spacing(2);
    let line = tokens.line;
    let pv = tokens.fill;
    let rule = container(space()).width(Length::Fill).height(Length::Fixed(1.0)).style(move |_theme: &Theme| {
        container::Style { background: Some(Background::Color(line)), ..container::Style::default() }
    });
    let table =
        container(rows).padding([10.0, 12.0]).width(Length::Fill).style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(pv)),
            border: Border { radius: 8.0.into(), ..Border::default() },
            ..container::Style::default()
        });
    column![rule, table].spacing(16).padding(Padding::ZERO.top(16.0)).into()
}

/// `StepMarks`: QR code, phone, account; done steps filled with the platform's colour.
fn steps<'a>(current: usize, platform: Platform, tokens: Tokens) -> Element<'a, Message> {
    let pc = folia_visualizer::color(Rgb::hex(platform.color), tokens.t1.a);
    let on = Color { a: tokens.t1.a, ..on_color(platform.color) };
    let mut marks = row![].align_y(Alignment::Center);
    for (index, glyph) in [Icon::QrCode, Icon::Smartphone, Icon::User].into_iter().enumerate() {
        if index > 0 {
            let reached = index <= current;
            let line = if reached { pc } else { tokens.line2 };
            marks = marks.push(container(space()).width(Length::Fixed(26.0)).height(Length::Fixed(1.5)).style(
                move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(line)),
                    ..container::Style::default()
                },
            ));
        }
        let done = index < current;
        let now = index == current;
        let ink = if done {
            on
        } else if now {
            tokens.t1
        } else {
            tokens.t3
        };
        let ring = if now { pc } else { tokens.line2 };
        marks =
            marks.push(container(icon(glyph, 13.0, ink)).center(Length::Fixed(26.0)).style(move |_theme: &Theme| {
                container::Style {
                    background: done.then_some(Background::Color(pc)),
                    border: Border { radius: 13.0.into(), width: if done { 0.0 } else { 1.5 }, color: ring },
                    ..container::Style::default()
                }
            }));
    }
    marks.into()
}

/// The login-done flood: the platform's colour fills the window from the tile, the account lands, then the window leaves.
fn flood<'a>(look: &Look<'a>, platform: Platform, ms: f64, opacity: f32, narrow: bool) -> Element<'a, Message> {
    let on = on_color(platform.color);
    let words = 1.0 - part(ms, DONE_LEAVE, DONE_LEAVE + 140.0, EASE_STANDARD);
    let picture_in = part(ms, 200.0, 340.0, EASE_STANDARD) * opacity;
    let landed = part(ms, 200.0, 640.0, EASE_IN_OUT);
    let line1 = part(ms, 250.0, 530.0, EASE_OUT) * words * opacity;
    let line2 = part(ms, 290.0, 610.0, EASE_OUT) * words * opacity;
    let tick = part(ms, 560.0, 740.0, EASE_OUT) * words * opacity;
    let watermark = part(ms, 220.0, 620.0, EASE_STANDARD) * words * opacity;
    let drift = 1.0 - part(ms, 140.0, 940.0, EASE_OUT);
    let surface =
        canvas_widget(Flood { color: platform.color, ms, opacity, narrow }).width(Length::Fill).height(Length::Fill);
    let name = match look.account {
        AccountState::SignedIn(account) => account_name(account, platform.short_name),
        _ => platform.short_name.to_owned(),
    };
    let letter = text(platform.letter.to_string())
        .size(196)
        .font(crate::typeface::font(iced::font::Weight::ExtraBold))
        .color(Color { a: 0.10 * watermark, ..on });
    let watermark = container(iced::widget::float(letter).translate(move |_, _| Vector::new(24.0 * drift, 0.0)))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::Center)
        .padding(Padding::ZERO.right(10.0));
    let picture: Element<'a, Message> = match look.avatar {
        Some(handle) => iced::widget::image(handle.clone())
            .width(Length::Fixed(88.0))
            .height(Length::Fixed(88.0))
            .border_radius(44.0)
            .opacity(picture_in)
            .into(),
        None => {
            let pc = folia_visualizer::color(Rgb::hex(platform.color), picture_in);
            let ring = Border { radius: 44.0.into(), width: 3.0, color: Color { a: picture_in, ..Color::WHITE } };
            match platform_icon(platform.id) {
                Some(handle) => container(
                    iced::widget::image(handle)
                        .width(Length::Fixed(82.0))
                        .height(Length::Fixed(82.0))
                        .border_radius(41.0)
                        .opacity(picture_in),
                )
                .center(Length::Fixed(88.0))
                .style(move |_theme: &Theme| container::Style { border: ring, ..container::Style::default() })
                .into(),
                None => container(
                    text(platform.letter.to_string())
                        .size(37)
                        .font(crate::typeface::font(iced::font::Weight::Bold))
                        .color(Color { a: picture_in, ..on }),
                )
                .center(Length::Fixed(88.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(pc)),
                    border: ring,
                    ..container::Style::default()
                })
                .into(),
            }
        }
    };
    let tick_badge =
        container(icon(Icon::Check, 14.0, Color { a: tick, ..folia_visualizer::color(Rgb::hex(platform.color), 1.0) }))
            .center(Length::Fixed(26.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(Color { a: tick, ..Color::WHITE })),
                border: Border { radius: 13.0.into(), ..Border::default() },
                ..container::Style::default()
            });
    let picture = stack![
        picture,
        container(tick_badge)
            .width(Length::Fixed(88.0))
            .height(Length::Fixed(88.0))
            .align_x(Alignment::End)
            .align_y(Alignment::End)
    ];
    let scale = 1.2 + (1.0 - 1.2) * landed;
    let lift = 40.0 * (1.0 - landed);
    let picture = iced::widget::float(picture).scale(scale).translate(move |_, _| Vector::new(0.0, -lift));
    let lines = column![
        iced::widget::float(
            text(look.i18n.t("home.loginSuccess")).size(13).font(MEDIUM).color(Color { a: line1, ..on })
        )
        .translate(move |_, _| Vector::new(0.0, 10.0 * (1.0 - line1))),
        iced::widget::float(
            text(name)
                .size(28)
                .font(SEMIBOLD)
                .color(Color { a: line2, ..on })
                .wrapping(iced::widget::text::Wrapping::None)
        )
        .translate(move |_, _| Vector::new(0.0, 12.0 * (1.0 - line2))),
    ]
    .spacing(2);
    let left = if narrow { 24.0 } else { 292.0 };
    let content = container(row![picture, lines].spacing(22).align_y(Alignment::Center))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .padding(Padding::ZERO.left(left));
    let shown = ms >= DONE_FRONT;
    if !shown {
        return surface.into();
    }
    stack![surface, watermark, content].into()
}

/// A part of a change from `from` to `to` ms at `t`, eased; 0 before, 1 after.
fn part(t: f64, from: f64, to: f64, ease: Ease) -> f32 {
    ease.at(((t - from) / (to - from)).clamp(0.0, 1.0)) as f32
}

/// The window's underlay: the waiting rings leaving the tile (`login-rings`, 4.2s, three staggered).
struct Underlay {
    pc: Color,
    rings: bool,
    clock: Instant,
    opacity: f32,
}

impl canvas::Program<Message> for Underlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.rings {
            let elapsed = self.clock.elapsed().as_secs_f64();
            let rip = Ease::new(0.25, 0.5, 0.5, 1.0);
            for delay in [0.0, 1.4, 2.8] {
                let t = ((elapsed + delay) / 4.2).fract();
                let eased = rip.at(t) as f32;
                let scale = 1.0 + 3.5 * eased;
                let alpha =
                    if t < 0.1 { 0.5 * (t / 0.1) as f32 } else { 0.5 * (1.0 - eased) / (1.0 - rip.at(0.1) as f32) };
                let ring = Path::circle(TILE_CENTER, 104.0 * scale);
                frame.stroke(
                    &ring,
                    Stroke::default()
                        .with_width(1.25)
                        .with_color(Color { a: alpha.clamp(0.0, 0.5) * self.opacity, ..self.pc }),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// `login-arc`: a 2px ring with one quarter in the platform's colour, turning once a second.
struct Arc {
    line: Color,
    pc: Color,
    turn: f32,
}

impl canvas::Program<Message> for Arc {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0 - 1.0;
        frame.stroke(&Path::circle(center, radius), Stroke::default().with_width(2.0).with_color(self.line));
        let start =
            -std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4 + self.turn.fract() * std::f32::consts::TAU;
        let arc = Path::new(|path| {
            path.arc(canvas::path::Arc {
                center,
                radius,
                start_angle: iced::Radians(start),
                end_angle: iced::Radians(start + std::f32::consts::FRAC_PI_2),
            });
        });
        frame.stroke(&arc, Stroke::default().with_width(2.0).with_color(self.pc));
        vec![frame.into_geometry()]
    }
}

/// The QR code, dark modules on the tile's white.
struct Code<'a> {
    qr: &'a LoginQr,
    dim: f32,
    opacity: f32,
    /// 0 hidden, 1 shown: a 135° edge with a soft 30% band sweeps from the top-left corner.
    reveal: f32,
}

impl canvas::Program<Message> for Code<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = self.qr.width.max(1);
        let module = bounds.width.min(bounds.height) / width as f32;
        let edge = -0.5 + 2.0 * self.reveal;
        for y in 0..width {
            for x in 0..width {
                if self.qr.dark(x, y) {
                    let diagonal = (x + y) as f32 / (2 * width) as f32;
                    let shown = ((edge - diagonal) / 0.3 + 0.5).clamp(0.0, 1.0);
                    let ink = Color { a: self.dim * self.opacity * shown, ..Color::BLACK };
                    frame.fill_rectangle(
                        Point::new(x as f32 * module, y as f32 * module),
                        Size::new(module + 0.2, module + 0.2),
                        ink,
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The done surface: the platform's colour as a 135° gradient from a lighter to a darker shade, growing from the tile.
struct Flood {
    color: u32,
    ms: f64,
    opacity: f32,
    narrow: bool,
}

impl canvas::Program<Message> for Flood {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = if self.narrow { Point::new(bounds.width / 2.0, 200.0) } else { TILE_CENTER };
        let far = [
            Point::ORIGIN,
            Point::new(bounds.width, 0.0),
            Point::new(0.0, bounds.height),
            Point::new(bounds.width, bounds.height),
        ]
        .into_iter()
        .map(|corner| corner.distance(center))
        .fold(0.0_f32, f32::max);
        let front = if self.ms < DONE_FRONT { 0.0 } else { part(self.ms, DONE_FRONT, 570.0, EASE_IN_OUT) };
        let radius = TILE / 2.0 + (far - TILE / 2.0) * front;
        let (light, dark) = shades(self.color);
        let base = folia_visualizer::color(Rgb::hex(self.color), self.opacity);
        let fill = gradient::Linear::new(Point::ORIGIN, Point::new(bounds.width, bounds.height))
            .add_stop(0.0, Color { a: self.opacity, ..light })
            .add_stop(0.46, base)
            .add_stop(1.0, Color { a: self.opacity, ..dark });
        frame.fill(&rounded_disc(center, radius.max(0.0), bounds.size(), 16.0), fill);
        vec![frame.into_geometry()]
    }
}

/// The disc as far as it lies inside the window's rounded rectangle: iced clips only to rectangles.
fn rounded_disc(center: Point, radius: f32, size: Size, corner: f32) -> Path {
    let inside = |point: Point| -> Point {
        let x = point.x.clamp(0.0, size.width);
        let y = point.y.clamp(0.0, size.height);
        let cx = x.clamp(corner, size.width - corner);
        let cy = y.clamp(corner, size.height - corner);
        let (dx, dy) = (x - cx, y - cy);
        let distance = (dx * dx + dy * dy).sqrt();
        if distance > corner {
            Point::new(cx + dx * corner / distance, cy + dy * corner / distance)
        } else {
            Point::new(x, y)
        }
    };
    Path::new(|path| {
        for step in 0..=128 {
            let angle = step as f32 / 128.0 * std::f32::consts::TAU;
            let point = inside(Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin()));
            if step == 0 { path.move_to(point) } else { path.line_to(point) }
        }
        path.close();
    })
}

/// `platformShades`: the lighter and darker ends, moving only OKLCH lightness.
fn shades(hex: u32) -> (Color, Color) {
    let to_linear =
        |channel: f32| if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) };
    let from_linear = |value: f32| {
        let value = value.clamp(0.0, 1.0);
        if value <= 0.003_130_8 { 12.92 * value } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 }
    };
    let (r, g, b) = (
        to_linear(((hex >> 16) & 0xff) as f32 / 255.0),
        to_linear(((hex >> 8) & 0xff) as f32 / 255.0),
        to_linear((hex & 0xff) as f32 / 255.0),
    );
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let lightness = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    let (chroma, hue) = (a.hypot(bb), bb.atan2(a));
    let shade = |lightness: f32, chroma: f32| {
        let (a, b) = (chroma * hue.cos(), chroma * hue.sin());
        let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        Color::from_rgb(
            from_linear(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
            from_linear(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
            from_linear(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
        )
    };
    (shade((lightness + 0.04).min(1.0), chroma), shade((lightness - 0.075).max(0.0), chroma + 0.01))
}
