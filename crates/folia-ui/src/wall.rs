// Home cover wall: the original Bravais wall of album tiles, ranked outward from the centre and panned by dragging.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use folia_core::{Rgb, Theme, Track};
use folia_library::Cover;
use iced::advanced::image::Handle;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text, gradient};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Task, Vector, mouse, window};

use crate::background::blocking;

const CELL: f32 = 128.0;
const GAP: f32 = 8.0;
const PITCH: f32 = CELL + GAP;
const BLOCK_COLS: i32 = 12;
const BLOCK_ROWS: i32 = 8;
const SLOTS_PER_BLOCK: usize = 12;
const RANK_VERTICAL_WEIGHT: f32 = 0.9;
const RANK_AREA_WEIGHT: f32 = 0.35;
const DRAG_THRESHOLD: f32 = 4.0;
const MAX_COVERS: usize = 120;
const TITLE_LINES: usize = 3;
const BADGE_FONT: Font = Font { weight: iced::font::Weight::ExtraBold, ..Font::DEFAULT };
const TITLE_FONT: Font = Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT };
const ARTIST_FONT: Font = Font { weight: iced::font::Weight::Semibold, ..Font::DEFAULT };

/// `x, y, cols, rows` in cells, from the original `blockTemplates.ts`.
const TEMPLATES: [[(i32, i32, i32, i32); SLOTS_PER_BLOCK]; 4] = [
    [
        (0, 0, 3, 2),
        (3, 0, 6, 4),
        (9, 0, 3, 2),
        (0, 2, 3, 2),
        (9, 2, 3, 4),
        (0, 4, 4, 4),
        (4, 4, 2, 2),
        (6, 4, 3, 2),
        (4, 6, 2, 2),
        (6, 6, 2, 2),
        (8, 6, 2, 2),
        (10, 6, 2, 2),
    ],
    [
        (0, 0, 2, 3),
        (2, 0, 2, 3),
        (4, 0, 2, 3),
        (6, 0, 6, 4),
        (0, 3, 6, 2),
        (6, 4, 2, 4),
        (8, 4, 2, 4),
        (10, 4, 2, 2),
        (0, 5, 2, 3),
        (2, 5, 2, 3),
        (4, 5, 2, 3),
        (10, 6, 2, 2),
    ],
    [
        (0, 0, 6, 4),
        (6, 0, 6, 3),
        (6, 3, 4, 3),
        (10, 3, 2, 2),
        (0, 4, 4, 2),
        (4, 4, 2, 2),
        (10, 5, 2, 3),
        (0, 6, 2, 2),
        (2, 6, 2, 2),
        (4, 6, 2, 2),
        (6, 6, 2, 2),
        (8, 6, 2, 2),
    ],
    [
        (0, 0, 2, 2),
        (2, 0, 2, 2),
        (4, 0, 4, 3),
        (8, 0, 2, 4),
        (10, 0, 2, 4),
        (0, 2, 2, 3),
        (2, 2, 2, 2),
        (4, 3, 4, 3),
        (2, 4, 2, 2),
        (8, 4, 4, 4),
        (0, 5, 2, 3),
        (2, 6, 6, 2),
    ],
];

/// One tile: where it sits on the wall (unscaled pixels) and which track it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    rect: Rectangle,
    track: usize,
}

/// A tile's text fitted to its width at the current scale: the title clamped to three lines, the artist to one.
#[derive(Debug, Clone)]
struct Label {
    title: String,
    artist: String,
}

/// Font sizes for one window width; labels are refitted when these change.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Typeset {
    scale: f32,
    title: f32,
    artist: f32,
    badge: f32,
    badge_box: Size,
}

impl Typeset {
    fn for_width(width: f32) -> Self {
        let scale = wall_scale(width);
        let badge = 11.0 * scale;
        let figures = folia_visualizer::measure("00", badge, BADGE_FONT, f32::INFINITY);
        Self {
            scale,
            title: (width * 0.02).clamp(22.0, 36.0) * scale,
            artist: 13.0 * scale,
            badge,
            badge_box: Size::new(figures.width + 16.0 * scale, figures.height + 10.0 * scale),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Viewport(Size),
    Pan(Vector),
    Settled,
    Play(usize),
    CoversLoaded(Vec<(i64, Option<Cover>)>),
}

#[derive(Default)]
pub struct Wall {
    placements: Vec<Placement>,
    labels: HashMap<usize, Label>,
    typeset: Option<Typeset>,
    viewport: Size,
    pan: Vector,
    covers: HashMap<i64, Option<Handle>>,
    order: VecDeque<i64>,
    pending: HashSet<i64>,
}

impl Wall {
    /// Lays out `tracks` around the wall's origin and fetches what lands on screen.
    pub fn relayout(&mut self, tracks: &[Track]) -> Task<Message> {
        self.placements = place(tracks.len());
        self.pan = Vector::ZERO;
        self.labels.clear();
        self.covers.clear();
        self.order.clear();
        self.show_visible(tracks)
    }

    pub fn update(&mut self, message: Message, tracks: &[Track]) -> (Task<Message>, Option<usize>) {
        match message {
            Message::Viewport(size) => {
                self.viewport = size;
                let typeset = Typeset::for_width(size.width);
                if self.typeset != Some(typeset) {
                    self.typeset = Some(typeset);
                    self.labels.clear();
                }
                (self.show_visible(tracks), None)
            }
            Message::Pan(delta) => {
                self.pan += delta;
                self.fit_visible(tracks);
                (Task::none(), None)
            }
            Message::Settled => (self.show_visible(tracks), None),
            Message::Play(index) => (Task::none(), Some(index)),
            Message::CoversLoaded(covers) => {
                for (id, cover) in covers {
                    self.pending.remove(&id);
                    let handle = cover.map(|c| Handle::from_rgba(c.width, c.height, c.rgba));
                    if handle.is_some() {
                        self.order.push_back(id);
                    }
                    self.covers.insert(id, handle);
                }
                self.evict();
                (Task::none(), None)
            }
        }
    }

    fn show_visible(&mut self, tracks: &[Track]) -> Task<Message> {
        self.fit_visible(tracks);
        self.load_visible(tracks)
    }

    /// Typesets the labels of tiles on screen that have none yet; measuring text is too slow to do every frame.
    fn fit_visible(&mut self, tracks: &[Track]) {
        let Some(typeset) = self.typeset else { return };
        let missing: Vec<Placement> =
            self.visible().filter(|placement| !self.labels.contains_key(&placement.track)).copied().collect();
        for placement in missing {
            let Some(track) = tracks.get(placement.track) else { continue };
            let width = placement.rect.width * typeset.scale - 34.0 * typeset.scale;
            let artist = track.artist.as_deref().unwrap_or_default();
            let label = Label {
                title: clamp_lines(&track.title, typeset.title, TITLE_FONT, width, TITLE_LINES),
                artist: clamp_lines(artist, typeset.artist, ARTIST_FONT, width, 1),
            };
            self.labels.insert(placement.track, label);
        }
    }

    /// Decodes covers for the tiles on screen that have none yet, on a worker thread, cropped to each tile.
    fn load_visible(&mut self, tracks: &[Track]) -> Task<Message> {
        let wanted: Vec<(i64, PathBuf, Size)> = self
            .visible()
            .filter_map(|placement| Some((tracks.get(placement.track)?, placement.rect.size())))
            .filter(|(track, _)| !self.covers.contains_key(&track.id) && !self.pending.contains(&track.id))
            .map(|(track, size)| (track.id, track.path.clone(), size))
            .collect();
        if wanted.is_empty() {
            return Task::none();
        }
        self.pending.extend(wanted.iter().map(|(id, ..)| *id));
        let work = blocking(move || {
            let cover =
                |path: &PathBuf, size: Size| folia_library::cover_for(path, size.width as u32, size.height as u32);
            wanted.into_iter().map(|(id, path, size)| (id, cover(&path, size))).collect()
        });
        Task::perform(work, |covers| Message::CoversLoaded(covers.unwrap_or_default()))
    }

    fn evict(&mut self) {
        while self.order.len() > MAX_COVERS {
            if let Some(old) = self.order.pop_front() {
                self.covers.remove(&old);
            }
        }
    }

    fn visible(&self) -> impl Iterator<Item = &Placement> {
        let (viewport, pan) = (self.viewport, self.pan);
        let scale = wall_scale(viewport.width);
        let screen = Rectangle::new(Point::ORIGIN, viewport);
        self.placements
            .iter()
            .filter(move |placement| to_screen(placement.rect, pan, viewport, scale).intersects(&screen))
    }

    pub fn view<'a>(
        &'a self,
        tracks: &'a [Track],
        theme: &'a Theme,
        daylight: bool,
        playing: Option<i64>,
    ) -> WallProgram<'a> {
        WallProgram { wall: self, tracks, theme, daylight, playing }
    }
}

pub struct WallProgram<'a> {
    wall: &'a Wall,
    tracks: &'a [Track],
    theme: &'a Theme,
    daylight: bool,
    playing: Option<i64>,
}

#[derive(Default)]
pub struct DragState {
    pressed_at: Option<Point>,
    last: Option<Point>,
    dragging: bool,
}

impl canvas::Program<Message> for WallProgram<'_> {
    type State = DragState;

    fn update(
        &self,
        state: &mut DragState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let scale = wall_scale(bounds.width);
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                (bounds.size() != self.wall.viewport).then(|| Action::publish(Message::Viewport(bounds.size())))
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                *state = DragState { pressed_at: Some(at), last: Some(at), dragging: false };
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (from, last) = (state.pressed_at?, state.last?);
                let at = cursor.position_in(bounds).unwrap_or(last);
                state.dragging |= from.distance(at) > DRAG_THRESHOLD;
                state.last = Some(at);
                state.dragging.then(|| Action::publish(Message::Pan((last - at) * (1.0 / scale))).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let from = state.pressed_at.take()?;
                if std::mem::take(&mut state.dragging) {
                    return Some(Action::publish(Message::Settled));
                }
                let pan = self.wall.pan;
                let hit = self.wall.visible().find(|p| to_screen(p.rect, pan, bounds.size(), scale).contains(from))?;
                (hit.track < self.tracks.len()).then(|| Action::publish(Message::Play(hit.track)))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (x * 60.0, y * 60.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                Some(Action::publish(Message::Pan(Vector::new(-dx, -dy) * (1.0 / scale))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &DragState,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let Some(typeset) = self.wall.typeset else { return vec![frame.into_geometry()] };
        let palette = Palette::new(self.theme, self.daylight);
        for placement in self.wall.visible() {
            let Some(track) = self.tracks.get(placement.track) else { continue };
            let tile = Tile {
                rect: to_screen(placement.rect, self.wall.pan, self.wall.viewport, typeset.scale),
                index: placement.track,
                cover: self.wall.covers.get(&track.id).and_then(Option::as_ref),
                label: self.wall.labels.get(&placement.track),
                current: self.playing == Some(track.id),
            };
            draw_tile(&mut frame, &tile, &typeset, &palette);
        }
        vec![frame.into_geometry()]
    }
}

/// The wall's colours from `wall.css`: ink on a poster ground, with a shade that is black at night and white by day.
struct Palette {
    ink: Color,
    accent: Color,
    poster: Color,
    shade: Color,
}

impl Palette {
    fn new(theme: &Theme, daylight: bool) -> Self {
        let poster_mix = if daylight { 0.06 } else { 0.12 };
        Self {
            ink: rgb(theme.primary, 1.0),
            accent: rgb(theme.accent, 1.0),
            poster: rgb(theme.background.mix(theme.primary, poster_mix), 1.0),
            shade: if daylight { Color::WHITE } else { Color::BLACK },
        }
    }

    fn shade(&self, alpha: f32) -> Color {
        Color { a: alpha, ..self.shade }
    }
}

struct Tile<'a> {
    rect: Rectangle,
    index: usize,
    cover: Option<&'a Handle>,
    label: Option<&'a Label>,
    current: bool,
}

// Everything is drawn in window coordinates: iced's `with_clip` does not move images and text into the clip.
fn draw_tile(frame: &mut Frame, tile: &Tile, typeset: &Typeset, palette: &Palette) {
    let Rectangle { x, y, width, height } = tile.rect;
    frame.fill_rectangle(tile.rect.position(), tile.rect.size(), palette.poster);
    if let Some(handle) = tile.cover {
        frame.draw_image(tile.rect, canvas::Image::new(handle.clone()));
    }
    let fade = gradient::Linear::new(Point::new(x, y), Point::new(x, y + height))
        .add_stop(0.0, palette.shade(0.05))
        .add_stop(0.34, palette.shade(0.05))
        .add_stop(0.56, palette.shade(0.24))
        .add_stop(1.0, palette.shade(0.92));
    frame.fill_rectangle(tile.rect.position(), tile.rect.size(), fade);
    draw_badge(frame, tile, typeset, palette);
    if let Some(label) = tile.label {
        draw_copy(frame, tile.rect, label, typeset, palette.ink);
    }
    if tile.current {
        frame.stroke(
            &Path::rectangle(Point::new(x + 1.0, y + 1.0), Size::new(width - 2.0, height - 2.0)),
            Stroke::default().with_width(2.0).with_color(palette.accent),
        );
    }
}

/// The track number in the top-left corner: 14px in, 11px heavy figures on a 62% shade (78% when playing).
fn draw_badge(frame: &mut Frame, tile: &Tile, typeset: &Typeset, palette: &Palette) {
    let inset = 14.0 * typeset.scale;
    let origin = Point::new(tile.rect.x + inset, tile.rect.y + inset);
    let size = typeset.badge_box;
    frame.fill_rectangle(origin, size, palette.shade(if tile.current { 0.78 } else { 0.62 }));
    frame.fill_text(Text {
        content: format!("{:02}", tile.index + 1),
        position: Point::new(origin.x + size.width / 2.0, origin.y + size.height / 2.0),
        color: if tile.current { palette.accent } else { palette.ink },
        size: typeset.badge.into(),
        font: BADGE_FONT,
        align_x: iced::widget::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..Text::default()
    });
}

/// Title above the artist (5px gap), both 17px in from the tile's bottom-left.
fn draw_copy(frame: &mut Frame, rect: Rectangle, label: &Label, typeset: &Typeset, ink: Color) {
    let inset = 17.0 * typeset.scale;
    let left = rect.x + inset;
    let width = (rect.width - inset * 2.0).max(1.0);
    let mut bottom = rect.y + rect.height - inset;
    if !label.artist.is_empty() {
        frame.fill_text(Text {
            content: label.artist.clone(),
            position: Point::new(left, bottom),
            max_width: width,
            color: Color { a: 0.78, ..ink },
            size: typeset.artist.into(),
            font: ARTIST_FONT,
            align_y: iced::alignment::Vertical::Bottom,
            ..Text::default()
        });
        bottom -= typeset.artist * 1.2 + 5.0 * typeset.scale;
    }
    frame.fill_text(Text {
        content: label.title.clone(),
        position: Point::new(left, bottom),
        max_width: width,
        color: ink,
        size: typeset.title.into(),
        line_height: iced::widget::text::LineHeight::Relative(0.96),
        font: TITLE_FONT,
        align_y: iced::alignment::Vertical::Bottom,
        ..Text::default()
    });
}

/// `content` cut with an ellipsis so it wraps to at most `lines` lines of `width`, like `-webkit-line-clamp`.
fn clamp_lines(content: &str, size: f32, font: Font, width: f32, lines: usize) -> String {
    let fits = |text: &str| {
        let bounds = folia_visualizer::measure(text, size, font, width);
        bounds.width <= width + 0.5 && (bounds.height / (size * 1.22)).round() as usize <= lines
    };
    if fits(content) {
        return content.to_owned();
    }
    let ends: Vec<usize> = content.char_indices().map(|(at, _)| at).collect();
    let cut = |count: usize| format!("{}\u{2026}", content[..ends[count]].trim_end());
    let (mut fitting, mut overflowing) = (0, ends.len());
    while overflowing - fitting > 1 {
        let middle = (fitting + overflowing) / 2;
        if fits(&cut(middle)) {
            fitting = middle;
        } else {
            overflowing = middle;
        }
    }
    cut(fitting)
}

/// The original `getBravaisScale`: the whole wall is drawn smaller on narrow windows.
fn wall_scale(viewport_width: f32) -> f32 {
    if viewport_width < 640.0 {
        0.52
    } else if viewport_width < 1100.0 {
        0.64
    } else {
        0.76
    }
}

fn to_screen(rect: Rectangle, pan: Vector, viewport: Size, scale: f32) -> Rectangle {
    let center = Point::new(viewport.width / 2.0, viewport.height / 2.0);
    Rectangle {
        x: center.x + (rect.x - pan.x) * scale,
        y: center.y + (rect.y - pan.y) * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}

fn rgb(color: Rgb, alpha: f32) -> Color {
    folia_visualizer::color(color, alpha)
}

/// Every slot of the blocks around the origin, ranked like the original `rankSlots` and filled in order.
fn place(count: usize) -> Vec<Placement> {
    if count == 0 {
        return Vec::new();
    }
    let mut radius = 0;
    while ((radius * 2 + 1) * (radius * 2 + 1)) as usize * SLOTS_PER_BLOCK < count * 2 {
        radius += 1;
    }
    let mut slots: Vec<(f32, Rectangle)> = Vec::new();
    for row in -radius..=radius {
        for column in -radius..=radius {
            for (x, y, cols, rows) in block(column, row) {
                let rect = Rectangle {
                    x: (column * BLOCK_COLS + x) as f32 * PITCH,
                    y: (row * BLOCK_ROWS + y) as f32 * PITCH,
                    width: cols as f32 * PITCH - GAP,
                    height: rows as f32 * PITCH - GAP,
                };
                let center = rect.center();
                let score = center.x.abs() + center.y.abs() * RANK_VERTICAL_WEIGHT
                    - ((cols * rows) as f32).sqrt() * PITCH * RANK_AREA_WEIGHT;
                slots.push((score, rect));
            }
        }
    }
    slots.sort_by(|a, b| a.0.total_cmp(&b.0));
    slots.into_iter().take(count).enumerate().map(|(track, (_, rect))| Placement { rect, track }).collect()
}

/// The block's template, picked and mirrored as the original `getBlockOrientation` does.
fn block(column: i32, row: i32) -> [(i32, i32, i32, i32); SLOTS_PER_BLOCK] {
    let template = TEMPLATES[(column + row * 2).rem_euclid(TEMPLATES.len() as i32) as usize];
    let reflection = mix(column, row) % 4;
    template.map(|(x, y, cols, rows)| {
        let x = if reflection & 1 == 1 { BLOCK_COLS - x - cols } else { x };
        let y = if reflection & 2 == 2 { BLOCK_ROWS - y - rows } else { y };
        (x, y, cols, rows)
    })
}

/// `mixBlockCoords` with JavaScript's 32-bit `Math.imul` and unsigned shifts.
fn mix(column: i32, row: i32) -> u32 {
    let mut value = (column.wrapping_mul(0x9e37_79b1_u32 as i32) ^ row.wrapping_mul(0x85eb_ca6b_u32 as i32)) as u32;
    value = ((value ^ (value >> 15)) as i32).wrapping_mul(0x2c1b_3c6d) as u32;
    value ^= value >> 12;
    value
}
