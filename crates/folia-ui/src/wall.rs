// Home cover wall: the original Bravais wall of tiles, repeated without an edge, ranked when filtered, or laid in explore lots.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use folia_core::{Rgb, Theme};
use folia_library::{Cover, CoverCache};
use folia_omni::Omni;
use iced::advanced::image::Handle;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text, gradient};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Task, Vector, mouse, window};

use crate::background::blocking;
use crate::lots::{self, LOT_COLS, LOT_ROWS, Lot};

const CELL: f32 = 128.0;
const GAP: f32 = 8.0;
const PITCH: f32 = CELL + GAP;
const BLOCK_COLS: i32 = 12;
const BLOCK_ROWS: i32 = 8;
const SLOTS_PER_BLOCK: usize = 12;
const RANK_VERTICAL_WEIGHT: f32 = 0.9;
const RANK_AREA_WEIGHT: f32 = 0.35;
const DRAG_THRESHOLD: f32 = 4.0;
/// Pixel budget of the decoded tile covers: three 1920×1080 screens of tiles at the 0.76 wall scale
/// (about 14 MB of RGBA each), so panning back and forth does not decode again.
const COVER_BYTES: usize = 48 * 1024 * 1024;
/// `COVER_SIZE_STEPS`: covers are asked for and decoded at the smallest step that covers the tile.
const COVER_STEPS: [u32; 3] = [256, 512, 1024];
/// `FIELD_ASPECT`: the repeat period of the edgeless wall is about this much wider than tall.
const FIELD_ASPECT: f32 = 2.2;
const TITLE_LINES: usize = 3;
const BADGE_FONT: Font = crate::typeface::font(iced::font::Weight::ExtraBold);
const TITLE_FONT: Font = crate::typeface::font(iced::font::Weight::Bold);
const ARTIST_FONT: Font = crate::typeface::font(iced::font::Weight::Semibold);

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

/// Explore lots stand apart by a street half a cell wide (`exploreLayout`).
const STREET: f32 = CELL / 2.0;
const LOT_PITCH_X: f32 = LOT_COLS as f32 * PITCH + STREET;
const LOT_PITCH_Y: f32 = LOT_ROWS as f32 * PITCH + STREET;

/// Where a tile's cover comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverSource {
    None,
    /// An audio file whose embedded or sidecar cover is read.
    Local(PathBuf),
    /// An online cover address, downloaded once into the cache by the facade.
    Remote(String),
}

/// What one tile shows: a song, a collection card or an explore heading.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Cover cache key and the id the playing outline matches.
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    /// The corner tag; `None` numbers the tile like the local wall does.
    pub badge: Option<String>,
    pub cover: CoverSource,
    /// An explore section's heading: no cover and no shade.
    pub heading: bool,
    /// Hue of the gradient drawn while there is no cover (`fallbackBackground` of the item key).
    pub hue: f32,
    /// A collection that opens into a list of songs: drawn with stacked page edges (`resolveBravaisTileForm`).
    pub stack: bool,
}

/// `fallbackBackground`: the hue is the sum of each code point's first UTF-16 unit (`charCodeAt(0)`), modulo 360.
pub fn key_hue(key: &str) -> f32 {
    (key.chars().map(|c| u32::from(c.encode_utf16(&mut [0; 2])[0])).sum::<u32>() % 360) as f32
}

/// How the wall lays its tiles: repeated without an edge, ranked round the origin (filtered), or explore lots.
pub enum Shape {
    Blocks,
    Finite,
    Explore(Vec<lots::Section>),
}

enum Layout {
    /// `getInfiniteSlotItem`: block slots in slot order over a period of whole blocks, rotated so `offset` is item 0.
    Blocks {
        count: usize,
        offset: usize,
        columns: i32,
        rows: i32,
    },
    Finite(Vec<Placement>),
    Explore(Vec<Lot>),
}

/// One tile: where it sits on the wall (unscaled pixels) and which tile it shows.
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
    /// A word tag and its box; numbered tiles use the typeset's box.
    badge: Option<(String, Size)>,
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
    /// A released drag. `velocity` is the pointer in screen px/s; `stale` skips the coast.
    Settled {
        velocity: Vector,
        stale: bool,
    },
    Play(usize),
    CoversLoaded(Vec<(i64, Option<Cover>)>),
}

pub struct Wall {
    layout: Layout,
    /// The placements on screen, rebuilt in place whenever the camera, seam or layout moves.
    shown: Vec<Placement>,
    /// Keyed by tile and the slot width it was fitted to: the edgeless wall shows one tile in slots of several sizes.
    labels: HashMap<(usize, u32), Label>,
    typeset: Option<Typeset>,
    viewport: Size,
    pan: Vector,
    covers: CoverCache<i64, Handle>,
    pending: HashSet<i64>,
    /// Remote cover downloads in flight; a new layout aborts them.
    downloads: Vec<iced::task::Handle>,
    seam_open: f32,
    anchor_x: f32,
    anchor_y: f32,
    horizontal: bool,
    planned: bool,
    coast: Option<Coast>,
}

/// Release coast from `wallPanMotion`: amplitude `0.3 × speed`, time constant 280ms.
struct Coast {
    origin: Vector,
    /// Pointer velocity in screen px/s, already capped.
    velocity: Vector,
    started: Instant,
}

impl Wall {
    pub fn new() -> Self {
        Self {
            layout: Layout::Finite(Vec::new()),
            shown: Vec::new(),
            labels: HashMap::new(),
            typeset: None,
            viewport: Size::ZERO,
            pan: Vector::ZERO,
            covers: CoverCache::new(COVER_BYTES),
            pending: HashSet::new(),
            downloads: Vec::new(),
            seam_open: 0.0,
            anchor_x: SEAM_ANCHOR,
            anchor_y: SEAM_ANCHOR,
            horizontal: false,
            planned: false,
            coast: None,
        }
    }

    /// Pans so a tile showing `index` sits in the middle of the wall: the copy nearest the camera.
    pub fn center_on(&mut self, index: usize) {
        let found = match &self.layout {
            Layout::Finite(placements) => placements.iter().find(|place| place.track == index).copied(),
            Layout::Explore(lots) => {
                let lot = lots.iter().position(|lot| lot.tiles.contains(&index));
                lot.and_then(|lot| {
                    let mut seats = Vec::new();
                    explore_lot(lots, 0, lot as i64, &mut seats);
                    seats.into_iter().find(|place| place.track == index)
                })
            }
            &Layout::Blocks { count, offset, columns, rows } => {
                let (block_w, block_h) = (BLOCK_COLS as f32 * PITCH, BLOCK_ROWS as f32 * PITCH);
                let (home_c, home_r) = ((self.pan.x / block_w).floor() as i32, (self.pan.y / block_h).floor() as i32);
                let mut best: Option<(f32, Placement)> = None;
                for row in home_r - rows..=home_r + rows {
                    for column in home_c - columns..=home_c + columns {
                        for (slot, rect) in block_rects(column, row).into_iter().enumerate() {
                            if wrap_item(column, row, slot, count, offset, columns, rows) != Some(index) {
                                continue;
                            }
                            let distance = rect.center().distance(Point::new(self.pan.x, self.pan.y));
                            if best.is_none_or(|(near, _)| distance < near) {
                                best = Some((distance, Placement { rect, track: index }));
                            }
                        }
                    }
                }
                best.map(|(_, place)| place)
            }
        };
        let Some(place) = found else { return };
        self.pan = Vector::new(place.rect.x + place.rect.width / 2.0, place.rect.y + place.rect.height / 2.0);
        self.refresh();
    }

    /// Lays out `tiles` around the wall's origin and fetches what lands on screen.
    pub fn relayout(&mut self, tiles: &[Tile], shape: Shape, omni: Option<&Omni>) -> Task<Message> {
        self.layout = match shape {
            Shape::Blocks => {
                let count = tiles.len();
                let (columns, rows) = wrap_period(count);
                // The start tile: the slot nearest the seam's far side shows item 0 (`getStartWrapOffset`).
                let start = nearest_slot(0, 0, Point::new(GAP, 0.0));
                let offset = if count == 0 { 0 } else { (count - start % count) % count };
                Layout::Blocks { count, offset, columns, rows }
            }
            Shape::Finite => Layout::Finite(place(tiles.len())),
            Shape::Explore(sections) => Layout::Explore(lots::strip(&sections)),
        };
        self.pan = Vector::ZERO;
        let (_, _, anchor) = self.grid();
        self.anchor_x = anchor;
        self.anchor_y = anchor;
        self.planned = false;
        self.coast = None;
        self.labels.clear();
        self.covers.clear();
        self.pending.clear();
        for download in self.downloads.drain(..) {
            download.abort();
        }
        self.refresh();
        self.show_visible(tiles, omni)
    }

    /// The explore layout's seam stands in the street between lot columns −1 and 0.
    fn grid(&self) -> (f32, f32, f32) {
        match self.layout {
            Layout::Explore(_) => (LOT_PITCH_X, LOT_PITCH_Y, -(GAP + STREET) / 2.0),
            Layout::Blocks { .. } | Layout::Finite(_) => {
                (BLOCK_COLS as f32 * PITCH, BLOCK_ROWS as f32 * PITCH, SEAM_ANCHOR)
            }
        }
    }

    pub fn update(&mut self, message: Message, tiles: &[Tile], omni: Option<&Omni>) -> (Task<Message>, Option<usize>) {
        match message {
            Message::Viewport(size) => {
                self.viewport = size;
                let typeset = Typeset::for_width(size.width);
                if self.typeset != Some(typeset) {
                    self.typeset = Some(typeset);
                    self.labels.clear();
                }
                self.refresh();
                (self.show_visible(tiles, omni), None)
            }
            Message::Pan(delta) => {
                self.pan_by(delta);
                (self.show_visible(tiles, omni), None)
            }
            Message::Settled { velocity, stale } => {
                self.release(velocity, stale);
                (self.show_visible(tiles, omni), None)
            }
            Message::Play(index) => (Task::none(), Some(index)),
            Message::CoversLoaded(covers) => {
                for (id, cover) in covers {
                    self.pending.remove(&id);
                    let bytes = cover.as_ref().map_or(0, |cover| cover.rgba.len());
                    let handle = cover.map(|cover| Handle::from_rgba(cover.width, cover.height, cover.rgba));
                    self.covers.insert(id, handle, bytes);
                }
                self.downloads.retain(|download| !download.is_aborted());
                (Task::none(), None)
            }
        }
    }

    /// Typesets and fetches what the camera now shows; the shell calls it once a pan settles or a coast ends.
    pub fn show_visible(&mut self, tiles: &[Tile], omni: Option<&Omni>) -> Task<Message> {
        self.fit_visible(tiles);
        self.load_visible(tiles, omni)
    }

    /// Typesets the labels of tiles on screen that have none yet; measuring text is too slow to do every frame.
    fn fit_visible(&mut self, tiles: &[Tile]) {
        let Some(typeset) = self.typeset else { return };
        for index in 0..self.shown.len() {
            let placement = self.shown[index];
            let key = (placement.track, placement.rect.width as u32);
            if self.labels.contains_key(&key) {
                continue;
            }
            let Some(tile) = tiles.get(placement.track) else { continue };
            let width = placement.rect.width * typeset.scale - 34.0 * typeset.scale;
            let badge = tile.badge.as_ref().map(|text| {
                let size = folia_visualizer::measure(text, typeset.badge, BADGE_FONT, f32::INFINITY);
                (text.clone(), Size::new(size.width + 16.0 * typeset.scale, typeset.badge_box.height))
            });
            let label = Label {
                title: folia_visualizer::clamp_lines(&tile.title, typeset.title, TITLE_FONT, width, TITLE_LINES),
                artist: folia_visualizer::clamp_lines(&tile.subtitle, typeset.artist, ARTIST_FONT, width, 1),
                badge,
            };
            self.labels.insert(key, label);
        }
    }

    /// Fetches covers for the tiles on screen that have none yet, nearest the middle of the window first.
    /// A cover is decoded square at a size step and cropped to each slot as it is drawn.
    fn load_visible(&mut self, tiles: &[Tile], omni: Option<&Omni>) -> Task<Message> {
        let scale = wall_scale(self.viewport.width);
        let middle = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
        let mut wanted: Vec<(f32, i64, CoverSource, u32)> = Vec::new();
        for placement in &self.shown {
            let Some(tile) = tiles.get(placement.track) else { continue };
            self.covers.touch(&tile.id);
            if tile.heading || self.covers.peek(&tile.id).is_some() || self.pending.contains(&tile.id) {
                continue;
            }
            if let Some(entry) = wanted.iter_mut().find(|entry| entry.1 == tile.id) {
                entry.3 = entry.3.max(cover_step(placement.rect.width.max(placement.rect.height) * scale));
                continue;
            }
            let distance = self.screen(placement.rect).center().distance(middle);
            let px = cover_step(placement.rect.width.max(placement.rect.height) * scale);
            wanted.push((distance, tile.id, tile.cover.clone(), px));
        }
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut tasks = Vec::new();
        let mut local = Vec::new();
        for (_, id, cover, px) in wanted {
            match cover {
                CoverSource::Local(path) => {
                    self.pending.insert(id);
                    local.push((id, path, px));
                }
                CoverSource::Remote(url) => {
                    let Some(omni) = omni.cloned() else {
                        self.covers.insert(id, None, 0);
                        continue;
                    };
                    self.pending.insert(id);
                    let (task, handle) = Task::perform(crate::background::online_cover(omni, url, px), move |cover| {
                        Message::CoversLoaded(vec![(id, cover)])
                    })
                    .abortable();
                    self.downloads.push(handle);
                    tasks.push(task);
                }
                CoverSource::None => self.covers.insert(id, None, 0),
            }
        }
        if !local.is_empty() {
            let work = blocking(move || {
                local.into_iter().map(|(id, path, px)| (id, folia_library::cover_for(&path, px, px))).collect()
            });
            tasks.push(Task::perform(work, |covers| Message::CoversLoaded(covers.unwrap_or_default())));
        }
        Task::batch(tasks)
    }

    /// Rebuilds the placements on screen in place: the edgeless and explore walls cut them from the blocks in view.
    fn refresh(&mut self) {
        let mut shown = std::mem::take(&mut self.shown);
        shown.clear();
        let screen = Rectangle::new(Point::ORIGIN, self.viewport);
        if self.viewport.width >= 1.0 {
            let scale = wall_scale(self.viewport.width).max(0.01);
            let reach = seam_push(self.seam().width, scale) / scale;
            let half =
                Vector::new(self.viewport.width / 2.0 / scale + reach, self.viewport.height / 2.0 / scale + reach);
            let (low, high) = (self.pan - half, self.pan + half);
            match &self.layout {
                Layout::Finite(placements) => shown.extend(placements.iter().copied()),
                Layout::Explore(lots) => {
                    for row in (low.y / LOT_PITCH_Y).floor() as i64..=(high.y / LOT_PITCH_Y).floor() as i64 {
                        for column in (low.x / LOT_PITCH_X).floor() as i64..=(high.x / LOT_PITCH_X).floor() as i64 {
                            explore_lot(lots, column, row, &mut shown);
                        }
                    }
                }
                &Layout::Blocks { count, offset, columns, rows } => {
                    let (block_w, block_h) = (BLOCK_COLS as f32 * PITCH, BLOCK_ROWS as f32 * PITCH);
                    for row in (low.y / block_h).floor() as i32..=(high.y / block_h).floor() as i32 {
                        for column in (low.x / block_w).floor() as i32..=(high.x / block_w).floor() as i32 {
                            for (slot, rect) in block_rects(column, row).into_iter().enumerate() {
                                if let Some(track) = wrap_item(column, row, slot, count, offset, columns, rows) {
                                    shown.push(Placement { rect, track });
                                }
                            }
                        }
                    }
                }
            }
            shown.retain(|placement| self.screen(placement.rect).intersects(&screen));
        }
        self.shown = shown;
    }
    /// A world rectangle on screen: scaled about the camera, and pushed aside by the open seam.
    fn screen(&self, rect: Rectangle) -> Rectangle {
        let scale = wall_scale(self.viewport.width);
        let push = seam_push(self.seam().width, scale);
        let mut on_screen = to_screen(rect, self.pan, self.viewport, scale);
        let anchor = if self.horizontal { self.anchor_y } else { self.anchor_x };
        let before = if self.horizontal { rect.center_y() < anchor } else { rect.center_x() < anchor };
        if self.horizontal {
            on_screen.y += if before { -push } else { push };
        } else {
            on_screen.x += if before { -push } else { push };
        }
        on_screen
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// The seam on screen. A viewport that has not been measured yet reports a closed seam.
    pub fn seam(&self) -> Seam {
        let view = self.viewport;
        if view.width < 1.0 || view.height < 1.0 {
            return Seam { center: 0.0, width: 0.0, collapsed: false, before: true, horizontal: self.horizontal };
        }
        let scale = wall_scale(view.width);
        if self.horizontal {
            seam_on_axis(self.anchor_y, self.pan.y, self.seam_open, view.height, view.height / 2.0, scale, true)
        } else {
            seam_on_axis(self.anchor_x, self.pan.x, self.seam_open, view.width, view.width / 2.0, scale, false)
        }
    }

    /// The width the seam opens to: the narrow seam or the spine, while it animates.
    pub fn set_seam(&mut self, width: f32) {
        if (width - self.seam_open).abs() > f32::EPSILON {
            self.seam_open = width;
            self.refresh();
        }
    }

    pub fn set_horizontal(&mut self, horizontal: bool) {
        if horizontal != self.horizontal {
            self.horizontal = horizontal;
            self.refresh();
        }
    }

    /// Picks a block boundary around the camera for an opening of `open` px, and yields the camera so it fits.
    pub fn replan(&mut self, open: f32) {
        let scale = wall_scale(self.viewport.width);
        let (block_x, block_y, anchor) = self.grid();
        if self.horizontal {
            let (line, extent) = (self.viewport.height / 2.0, self.viewport.height);
            let (anchor, camera) = plan_on_grid(self.pan.y, open, extent, line, scale, block_y, anchor);
            self.anchor_y = anchor;
            self.pan.y = camera;
        } else {
            let (line, extent) = (self.viewport.width / 2.0, self.viewport.width);
            let (anchor, camera) = plan_on_grid(self.pan.x, open, extent, line, scale, block_x, anchor);
            self.anchor_x = anchor;
            self.pan.x = camera;
        }
        self.planned = true;
        self.refresh();
    }

    /// Plans the first opening once the wall has a viewport and a real width.
    pub fn plan_if_needed(&mut self) {
        if self.planned || self.viewport.width < 1.0 || self.seam_open < 1.0 {
            return;
        }
        self.replan(self.seam_open);
    }

    pub fn coasting(&self) -> bool {
        self.coast.is_some()
    }

    /// Steps the release coast. Returns whether the camera moved.
    pub fn tick_coast(&mut self, now: Instant) -> bool {
        let Some(coast) = &self.coast else { return false };
        let (origin, velocity, started) = (coast.origin, coast.velocity, coast.started);
        let speed = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        if speed < 1.0 {
            self.coast = None;
            return false;
        }
        let t = now.saturating_duration_since(started).as_secs_f32() * 1000.0;
        let amplitude = speed * COAST_POWER;
        let remaining = amplitude * (-t / COAST_MS).exp();
        let distance = amplitude - remaining;
        let scale = wall_scale(self.viewport.width).max(0.01);
        let along = Vector::new(-velocity.x / speed, -velocity.y / speed) * (distance / scale);
        self.pan = origin + along;
        if remaining < COAST_REST {
            self.coast = None;
        }
        self.refresh();
        true
    }

    /// The home wall is infinite, so a drag is not resisted. A new drag cancels the coast.
    fn pan_by(&mut self, delta: Vector) {
        self.coast = None;
        self.pan += delta;
        self.refresh();
    }

    fn release(&mut self, velocity: Vector, stale: bool) {
        let speed = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        if stale || speed < COAST_MIN {
            self.coast = None;
            return;
        }
        let capped = speed.min(COAST_MAX);
        self.coast = Some(Coast {
            origin: self.pan,
            velocity: Vector::new(velocity.x * capped / speed, velocity.y * capped / speed),
            started: Instant::now(),
        });
    }

    pub fn view<'a>(
        &'a self,
        tracks: &'a [Tile],
        theme: &'a Theme,
        daylight: bool,
        playing: Option<i64>,
    ) -> WallProgram<'a> {
        WallProgram { wall: self, tracks, theme, daylight, playing }
    }
}

/// The placements of the lot the place `(column, row)` holds: its heading and its seats.
fn explore_lot(lots: &[Lot], column: i64, row: i64, into: &mut Vec<Placement>) {
    let Some(lot) = lots.get(lots::lot_at(column, row, lots.len())) else { return };
    let origin = Point::new(column as f32 * LOT_PITCH_X, row as f32 * LOT_PITCH_Y);
    let rect = |(x, y, cols, rows): lots::Seat| Rectangle {
        x: origin.x + x as f32 * PITCH,
        y: origin.y + y as f32 * PITCH,
        width: cols as f32 * PITCH - GAP,
        height: rows as f32 * PITCH - GAP,
    };
    into.push(Placement { rect: rect(lot.heading), track: lot.heading_tile });
    into.extend(lot.tiles.iter().zip(&lot.seats).map(|(&track, &seat)| Placement { rect: rect(seat), track }));
}

/// `resolveCoverSizeStep`: the smallest step covering `px`, the largest past it.
fn cover_step(px: f32) -> u32 {
    COVER_STEPS.into_iter().find(|step| *step as f32 >= px).unwrap_or(COVER_STEPS[COVER_STEPS.len() - 1])
}

/// `getWrapPeriod`: blocks per row and block rows of the repeat, from `getLatticeGeometry`.
fn wrap_period(count: usize) -> (i32, i32) {
    let blocks = count.max(1).div_ceil(SLOTS_PER_BLOCK) as f32;
    let columns = if blocks <= 1.0 {
        1
    } else {
        ((FIELD_ASPECT * blocks * BLOCK_ROWS as f32 / BLOCK_COLS as f32).sqrt().round() as i32).max(1)
    };
    (columns, (blocks / columns as f32).ceil() as i32)
}

/// `getInfiniteSlotItem`: the item a slot shows, `None` for an empty wall.
fn wrap_item(
    column: i32,
    row: i32,
    slot: usize,
    count: usize,
    offset: usize,
    columns: i32,
    rows: i32,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let block = (row.rem_euclid(rows) * columns + column.rem_euclid(columns)) as usize;
    Some((block * SLOTS_PER_BLOCK + slot + offset) % count)
}

/// A block's slot rectangles in slot order, in unscaled wall pixels.
fn block_rects(column: i32, row: i32) -> [Rectangle; SLOTS_PER_BLOCK] {
    block(column, row).map(|(x, y, cols, rows)| Rectangle {
        x: (column * BLOCK_COLS + x) as f32 * PITCH,
        y: (row * BLOCK_ROWS + y) as f32 * PITCH,
        width: cols as f32 * PITCH - GAP,
        height: rows as f32 * PITCH - GAP,
    })
}

/// `startKeyAt`: the slot of a block whose centre is nearest `point`.
fn nearest_slot(column: i32, row: i32, point: Point) -> usize {
    block_rects(column, row)
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.center().distance(point).total_cmp(&b.1.center().distance(point)))
        .map_or(0, |(slot, _)| slot)
}

pub struct WallProgram<'a> {
    wall: &'a Wall,
    tracks: &'a [Tile],
    theme: &'a Theme,
    daylight: bool,
    playing: Option<i64>,
}

#[derive(Default)]
pub struct DragState {
    pressed_at: Option<Point>,
    last: Option<Point>,
    dragging: bool,
    moved_at: Option<Instant>,
    /// Pointer velocity in screen px/s.
    velocity: Vector,
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
                *state = DragState {
                    pressed_at: Some(at),
                    last: Some(at),
                    dragging: false,
                    moved_at: Some(Instant::now()),
                    velocity: Vector::ZERO,
                };
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (from, last) = (state.pressed_at?, state.last?);
                let at = cursor.position_in(bounds).unwrap_or(last);
                state.dragging |= from.distance(at) > DRAG_THRESHOLD;
                let now = Instant::now();
                if let Some(then) = state.moved_at {
                    let dt = now.saturating_duration_since(then).as_secs_f32();
                    if dt > 0.0 {
                        state.velocity = Vector::new((at.x - last.x) / dt, (at.y - last.y) / dt);
                    }
                }
                state.moved_at = Some(now);
                state.last = Some(at);
                state.dragging.then(|| Action::publish(Message::Pan((last - at) * (1.0 / scale))).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let from = state.pressed_at.take()?;
                if std::mem::take(&mut state.dragging) {
                    let stale = state.moved_at.is_none_or(|at| at.elapsed().as_millis() > u128::from(COAST_STALE_MS));
                    let velocity = state.velocity;
                    state.velocity = Vector::ZERO;
                    return Some(Action::publish(Message::Settled { velocity, stale }));
                }

                let hit = self.wall.shown.iter().rev().find(|p| self.wall.screen(p.rect).contains(from))?;
                let tile = self.tracks.get(hit.track)?;
                (!tile.heading).then(|| Action::publish(Message::Play(hit.track)))
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
        for placement in &self.wall.shown {
            let Some(item) = self.tracks.get(placement.track) else { continue };
            let tile = Drawn {
                rect: self.wall.screen(placement.rect),
                index: placement.track,
                cover: self.wall.covers.peek(&item.id).and_then(Option::as_ref),
                label: self.wall.labels.get(&(placement.track, placement.rect.width as u32)),
                current: self.playing == Some(item.id),
                heading: item.heading,
                hue: item.hue,
                stack: item.stack,
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
    page_near: Color,
    page_far: Color,
    page_line: Color,
}

impl Palette {
    fn new(theme: &Theme, daylight: bool) -> Self {
        let poster_mix = if daylight { 0.06 } else { 0.12 };
        // `.bravais-tile-face.is-stack`: the page colours mix the background with the poster ink.
        let (near, far, line_ink, line) =
            if daylight { (0.20, 0.12, theme.primary, 0.38) } else { (0.34, 0.20, Rgb { r: 0, g: 0, b: 0 }, 0.28) };
        Self {
            ink: rgb(theme.primary, 1.0),
            accent: rgb(theme.accent, 1.0),
            poster: rgb(theme.background.mix(theme.primary, poster_mix), 1.0),
            shade: if daylight { Color::WHITE } else { Color::BLACK },
            page_near: rgb(theme.background.mix(theme.primary, near), 1.0),
            page_far: rgb(theme.background.mix(theme.primary, far), 1.0),
            page_line: rgb(theme.background.mix(line_ink, line), 1.0),
        }
    }

    fn shade(&self, alpha: f32) -> Color {
        Color { a: alpha, ..self.shade }
    }
}

struct Drawn<'a> {
    rect: Rectangle,
    index: usize,
    cover: Option<&'a Handle>,
    label: Option<&'a Label>,
    current: bool,
    heading: bool,
    hue: f32,
    stack: bool,
}

// Everything is drawn in window coordinates: iced's `with_clip` does not move images and text into the clip.
fn draw_tile(frame: &mut Frame, tile: &Drawn, typeset: &Typeset, palette: &Palette) {
    let Rectangle { x, y, width, height } = tile.rect;
    if tile.stack {
        // `has-stack-edges`: pages 6px and 3px down-right, each with a 1px line round it, under the poster.
        for (offset, page) in [(6.0, palette.page_far), (3.0, palette.page_near)] {
            frame.fill_rectangle(
                Point::new(x + offset - 1.0, y + offset - 1.0),
                Size::new(width + 2.0, height + 2.0),
                palette.page_line,
            );
            frame.fill_rectangle(Point::new(x + offset, y + offset), tile.rect.size(), page);
        }
    }
    frame.fill_rectangle(tile.rect.position(), tile.rect.size(), palette.poster);
    if tile.heading {
        draw_badge(frame, tile, typeset, palette);
        if let Some(label) = tile.label {
            draw_copy(frame, tile.rect, label, typeset, palette.ink);
        }
        return;
    }
    let fallback = gradient::Linear::new(Point::new(x, y), Point::new(x + width, y + height))
        .add_stop(0.0, hsl(tile.hue, 0.68, 0.58))
        .add_stop(1.0, hsl((tile.hue + 52.0) % 360.0, 0.62, 0.18));
    frame.fill_rectangle(tile.rect.position(), tile.rect.size(), fallback);
    if let Some(handle) = tile.cover {
        // `background-size: cover` of a square cover: drawn over the longer side, clipped to the slot.
        let side = width.max(height);
        let square = Rectangle { x: x + (width - side) / 2.0, y: y + (height - side) / 2.0, width: side, height: side };
        frame.with_clip(tile.rect, |clipped| clipped.draw_image(square, canvas::Image::new(handle.clone())));
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
fn draw_badge(frame: &mut Frame, tile: &Drawn, typeset: &Typeset, palette: &Palette) {
    let inset = 14.0 * typeset.scale;
    let origin = Point::new(tile.rect.x + inset, tile.rect.y + inset);
    let word = tile.label.and_then(|label| label.badge.as_ref());
    let size = word.map_or(typeset.badge_box, |(_, size)| *size);
    frame.fill_rectangle(origin, size, palette.shade(if tile.current { 0.78 } else { 0.62 }));
    frame.fill_text(Text {
        content: word.map_or_else(|| format!("{:02}", tile.index + 1), |(text, _)| text.clone()),
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

/// CSS `hsl()`.
fn hsl(hue: f32, saturation: f32, lightness: f32) -> Color {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lightness - chroma / 2.0;
    Color::from_rgb(r + base, g + base, b + base)
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

/// The seam opens on the boundary between block columns -1 and 0, the middle of the first gap.
pub const SEAM_ANCHOR: f32 = -GAP / 2.0;
/// `SEAM_EDGE_MARGIN`: the seam starts closing this far from the window edge.
const SEAM_EDGE: f32 = 14.0;
/// `SEAM_LINE_DISTANCE_WEIGHT`.
const LINE_WEIGHT: f32 = 0.15;
const COAST_MIN: f32 = 40.0;
const COAST_MAX: f32 = 4000.0;
const COAST_POWER: f32 = 0.3;
const COAST_MS: f32 = 280.0;
const COAST_REST: f32 = 0.5;
const COAST_STALE_MS: u32 = 80;

/// Where the seam is on screen: its centre, how wide it is open, and whether the edge shut it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seam {
    pub center: f32,
    pub width: f32,
    /// Meant to be open but squeezed shut (`getSeamGeometry` `collapsed`).
    pub collapsed: bool,
    /// The centre is before the base line, so a vertical tab sits on the left.
    pub before: bool,
    pub horizontal: bool,
}

/// `getSeamGeometry` for a vertical seam anchored on column 0. `camera` is the world point under the base line.
pub fn seam_geometry(camera: f32, open_width: f32, viewport_width: f32) -> Seam {
    let scale = wall_scale(viewport_width);
    seam_on_axis(SEAM_ANCHOR, camera, open_width, viewport_width, viewport_width / 2.0, scale, false)
}

/// `geometryOnAxis`: the anchor follows the camera and narrows within `SEAM_EDGE` of either end.
pub fn seam_on_axis(
    anchor: f32,
    camera: f32,
    open_width: f32,
    extent: f32,
    line: f32,
    scale: f32,
    horizontal: bool,
) -> Seam {
    let center = line + (anchor - camera) * scale;
    let room = center.min(extent - center) - SEAM_EDGE;
    let width = open_width.min(room * 2.0).max(0.0);
    Seam { center, width, collapsed: open_width > 1.0 && width < 2.0, before: center < line, horizontal }
}

/// `(开口 / 排版宽度 − 0.35) / 0.65`, zero once the opening is under 35% of the laid-out width.
pub fn content_opacity(open: f32, layout: f32) -> f32 {
    ((open / layout.max(1.0) - 0.35) / 0.65).clamp(0.0, 1.0)
}

/// The wider side of a seam, or the whole extent when the seam is shut. Equal sides take the far one.
pub fn wider_span(center: f32, width: f32, extent: f32) -> (f32, f32) {
    if width < 1.0 || extent <= 0.0 {
        return (0.0, extent.max(0.0));
    }
    let near = (center - width / 2.0).clamp(0.0, extent);
    let far = (center + width / 2.0).clamp(0.0, extent);
    let far_room = extent - far;
    if far_room >= near { (far, far_room) } else { (0.0, near) }
}

/// `blockSeamPlan`: the boundary near `camera`, and the camera after the smallest yield that fits `open`.
pub fn plan_opening(camera: f32, open: f32, extent: f32, line: f32, scale: f32, block: f32) -> (f32, f32) {
    plan_on_grid(camera, open, extent, line, scale, block, SEAM_ANCHOR)
}

/// `blockSeamPlan` on a grid whose boundaries sit at `index × block + anchor`.
fn plan_on_grid(camera: f32, open: f32, extent: f32, line: f32, scale: f32, block: f32, anchor: f32) -> (f32, f32) {
    if block <= 1.0 || scale <= 0.0 || extent <= 0.0 {
        return (anchor, camera);
    }
    let need = open / 2.0 + SEAM_EDGE;
    let base = js_round((camera - anchor) / block);
    let mut best = (anchor, camera);
    let mut best_score = f32::INFINITY;
    for index in (base - 1)..=(base + 1) {
        let at = index as f32 * block + anchor;
        let screen = line + (at - camera) * scale;
        let fit = need.max((extent - need).min(screen));
        let next = at - (fit - line) / scale;
        let shift = (next - camera).abs() * scale;
        let score = shift + (fit - line).abs() * LINE_WEIGHT;
        if score < best_score {
            best = (at, next);
            best_score = score;
        }
    }
    best
}

/// `Math.round`: halves go towards +∞, unlike Rust's half-away-from-zero.
fn js_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

/// `getSeamSplitOffsets`: how far the halves of the wall move apart, on screen, for an opening this wide.
fn seam_push(width: f32, scale: f32) -> f32 {
    width / 2.0 + GAP / 2.0 * scale * (width / 16.0).min(1.0)
}
