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
use crate::focus_card::{self, BlockShape, Focus, SlotId};
use crate::lots::{self, LOT_COLS, LOT_ROWS, Lot};

const CELL: f32 = 128.0;
const GAP: f32 = 8.0;
const PITCH: f32 = CELL + GAP;
const BLOCK_COLS: i32 = 12;
const BLOCK_ROWS: i32 = 8;
const SLOTS_PER_BLOCK: usize = 12;
/// A block's width on the wall (`BLOCK_COLS · pitch − gap`), the span the focus reflow's spring is sampled for.
pub const BLOCK_WIDTH: f32 = BLOCK_COLS as f32 * PITCH - GAP;
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
    slot: SlotId,
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
    /// A click on a tile (`activate`), or on the wall where no tile is.
    Press(Option<(usize, SlotId)>),
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
    /// The focus card and the keyboard focus (`useBravaisFocus`).
    pub focus: Focus,
    /// The album wall's mosaic or shards while an album poster is on screen (`has-album-wall`).
    pub album: Option<crate::album_wall::AlbumWall>,
    /// The layer turning with a poster on screen.
    pub tilt: crate::poster_motion::Tilt,
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
            focus: Focus::default(),
            album: None,
            tilt: crate::poster_motion::Tilt::new(Instant::now()),
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
                                let slot =
                                    SlotId { column: i64::from(column), row: i64::from(row), index: slot, lot: false };
                                best = Some((distance, Placement { rect, track: index, slot }));
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
        self.focus.reset();
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

    pub fn update(
        &mut self,
        message: Message,
        tiles: &[Tile],
        omni: Option<&Omni>,
    ) -> (Task<Message>, Option<Option<(usize, SlotId)>>) {
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
                self.focus.stop_camera();
                self.pan_by(delta);
                (self.show_visible(tiles, omni), None)
            }
            Message::Settled { velocity, stale } => {
                self.release(velocity, stale);
                (self.show_visible(tiles, omni), None)
            }
            Message::Press(hit) => (Task::none(), Some(hit)),
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
        for index in 0..self.shown.len() * 2 {
            let placement = self.shown[index / 2];
            // Each tile at rest, and where the focus card's reflow takes it.
            let width = match index % 2 {
                0 => placement.rect.width,
                _ => match self.focus.target(placement.slot) {
                    Some(target) => target.width,
                    None => continue,
                },
            };
            let key = (placement.track, width as u32);
            if self.labels.contains_key(&key) {
                continue;
            }
            let Some(tile) = tiles.get(placement.track) else { continue };
            let width = width * typeset.scale - 34.0 * typeset.scale;
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
                            for (index, rect) in block_rects(column, row).into_iter().enumerate() {
                                if let Some(track) = wrap_item(column, row, index, count, offset, columns, rows) {
                                    let slot =
                                        SlotId { column: i64::from(column), row: i64::from(row), index, lot: false };
                                    shown.push(Placement { rect, track, slot });
                                }
                            }
                        }
                    }
                }
            }
            let now = Instant::now();
            shown.retain(|placement| {
                self.screen(placement.rect).intersects(&screen)
                    || self.focus.involves(placement.slot) && {
                        self.screen(self.focus.rect(placement.slot, placement.rect, now)).intersects(&screen)
                    }
            });
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
        band: Option<crate::poster::Band<'a>>,
    ) -> WallProgram<'a> {
        WallProgram { wall: self, tracks, theme, daylight, playing, band }
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
    let slot = |index: usize| SlotId { column, row, index, lot: true };
    into.push(Placement { rect: rect(lot.heading), track: lot.heading_tile, slot: slot(lot.seats.len()) });
    into.extend(lot.tiles.iter().zip(&lot.seats).enumerate().map(|(index, (&track, &seat))| Placement {
        rect: rect(seat),
        track,
        slot: slot(index),
    }));
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
    /// The poster's ground, drawn into the tilted seam with the walls while the layer turns.
    band: Option<crate::poster::Band<'a>>,
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

                // The grown card is on top, then the focused tile, then the rest in drawing order; a tilted layer is
                // turned back first.
                let now = Instant::now();
                let from = self.wall.untilt(from, now);
                let expanded = self.wall.focus.expanded().map(|(slot, _)| slot);
                let keyboard = self.wall.focus.keyboard();
                let hit = self
                    .wall
                    .shown
                    .iter()
                    .filter(|p| Some(p.slot) == expanded)
                    .chain(self.wall.shown.iter().filter(|p| Some(p.slot) == keyboard && Some(p.slot) != expanded))
                    .chain(self.wall.shown.iter().rev())
                    .find(|p| self.wall.drawn(p, now).contains(from));
                let Some(hit) = hit else { return Some(Action::publish(Message::Press(None))) };
                let tile = self.tracks.get(hit.track)?;
                (!tile.heading).then(|| Action::publish(Message::Press(Some((hit.track, hit.slot)))))
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
        let now = Instant::now();
        // `withTilt`: both halves and the seam turn about the poster's right edge at half the window's height.
        let deg = self.wall.tilt.deg(now);
        if deg.abs() > 0.001 {
            let pivot = self.wall.pivot();
            frame.translate(Vector::new(pivot.x, pivot.y));
            frame.rotate(deg.to_radians());
            frame.translate(Vector::new(-pivot.x, -pivot.y));
            if let Some(band) = self.band {
                self.wall.draw_band(&mut frame, band);
            }
        }
        let expanded = self.wall.focus.expanded().map(|(slot, _)| slot);
        let keyboard = self.wall.focus.keyboard();
        let raised = |placement: &&Placement| Some(placement.slot) == expanded || Some(placement.slot) == keyboard;
        // `z-index`: the rest, then the keyboard focus (4), then the grown card (20).
        let order = self
            .wall
            .shown
            .iter()
            .filter(|placement| !raised(placement))
            .chain(self.wall.shown.iter().filter(|p| Some(p.slot) == keyboard && Some(p.slot) != expanded))
            .chain(self.wall.shown.iter().filter(|p| Some(p.slot) == expanded));
        for placement in order {
            let Some(item) = self.tracks.get(placement.track) else { continue };
            let rect = self.wall.drawn(placement, now);
            let world_width = self.wall.focus.rect(placement.slot, placement.rect, now).width;
            let label = [
                world_width,
                self.wall.focus.target(placement.slot).map_or(placement.rect.width, |t| t.width),
                placement.rect.width,
            ]
            .into_iter()
            .find_map(|width| self.wall.labels.get(&(placement.track, width as u32)));
            let grown = Some(placement.slot) == expanded;
            let art = if grown || item.heading {
                Art::None
            } else {
                self.wall.art(placement, item, rect, Some(placement.slot) == keyboard || self.playing == Some(item.id))
            };
            let tile = Drawn {
                rect,
                index: placement.track,
                cover: self.wall.covers.peek(&item.id).and_then(Option::as_ref),
                art,
                label: if grown { None } else { label },
                current: self.playing == Some(item.id),
                heading: item.heading,
                hue: item.hue,
                stack: item.stack,
            };
            draw_tile(&mut frame, &tile, &typeset, &palette);
            if grown {
                draw_grown_shade(&mut frame, rect, &palette);
            }
            if Some(placement.slot) == keyboard {
                draw_focus(&mut frame, rect, tile.current, self.daylight, &palette);
            }
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
    art: Art<'a>,
    label: Option<&'a Label>,
    current: bool,
    heading: bool,
    hue: f32,
    stack: bool,
}

/// The album wall's art in a tile's place: its share of the block's cover, or a zoomed duotone shard with dots.
enum Art<'a> {
    None,
    Mosaic { image: &'a Handle, square: Rectangle },
    Shard { image: &'a Handle, rect: Rectangle, dots: Option<&'a Handle> },
}

// Everything is drawn in window coordinates: iced's `with_clip` does not move images and text into the clip.
fn draw_tile(frame: &mut Frame, tile: &Drawn, typeset: &Typeset, palette: &Palette) {
    let Rectangle { x, y, width, height } = tile.rect;
    if tile.stack {
        // `has-stack-edges`: pages 6px and 3px down-right, each with a 1px line round it, under the poster.
        for (offset, page) in [(6.0, palette.page_far), (3.0, palette.page_near)] {
            frame.fill(
                &Path::rectangle(Point::new(x + offset - 1.0, y + offset - 1.0), Size::new(width + 2.0, height + 2.0)),
                palette.page_line,
            );
            frame.fill(&Path::rectangle(Point::new(x + offset, y + offset), tile.rect.size()), page);
        }
    }
    frame.fill(&Path::rectangle(tile.rect.position(), tile.rect.size()), palette.poster);
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
    frame.fill(&Path::rectangle(tile.rect.position(), tile.rect.size()), fallback);
    let mosaic = matches!(tile.art, Art::Mosaic { .. });
    match tile.art {
        Art::Mosaic { image, square } => {
            frame.with_clip(tile.rect, |clipped| clipped.draw_image(square, canvas::Image::new(image.clone())));
        }
        Art::Shard { image, rect, dots } => {
            frame.with_clip(tile.rect, |clipped| {
                clipped.draw_image(rect, canvas::Image::new(image.clone()));
                if let Some(dots) = dots {
                    let side = crate::album_wall::DOTS_SIDE as f32;
                    let mut row = y;
                    while row < y + height {
                        let mut column = x;
                        while column < x + width {
                            let cell = Rectangle::new(Point::new(column, row), Size::new(side, side));
                            clipped.draw_image(cell, canvas::Image::new(dots.clone()));
                            column += side;
                        }
                        row += side;
                    }
                }
            });
        }
        Art::None => {
            if let Some(handle) = tile.cover {
                // `background-size: cover` of a square cover: drawn over the longer side, clipped to the slot.
                let side = width.max(height);
                let square =
                    Rectangle { x: x + (width - side) / 2.0, y: y + (height - side) / 2.0, width: side, height: side };
                frame.with_clip(tile.rect, |clipped| clipped.draw_image(square, canvas::Image::new(handle.clone())));
            }
        }
    }
    // A mosaic tile keeps only a light shade at its foot, so the pieces read as one cover; its copy stands on a shadow.
    let fade = if mosaic {
        gradient::Linear::new(Point::new(x, y), Point::new(x, y + height))
            .add_stop(0.55, palette.shade(0.0))
            .add_stop(0.74, palette.shade(0.12))
            .add_stop(1.0, palette.shade(0.40))
    } else {
        gradient::Linear::new(Point::new(x, y), Point::new(x, y + height))
            .add_stop(0.0, palette.shade(0.05))
            .add_stop(0.34, palette.shade(0.05))
            .add_stop(0.56, palette.shade(0.24))
            .add_stop(1.0, palette.shade(0.92))
    };
    frame.fill(&Path::rectangle(tile.rect.position(), tile.rect.size()), fade);
    draw_badge(frame, tile, typeset, palette);
    if let Some(label) = tile.label {
        if mosaic {
            let below = Rectangle { y: tile.rect.y + 1.0, ..tile.rect };
            draw_copy(frame, below, label, typeset, palette.shade(0.6));
        }
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
    frame.fill(&Path::rectangle(origin, size), palette.shade(if tile.current { 0.78 } else { 0.62 }));
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
    let mut slots: Vec<(f32, Rectangle, SlotId)> = Vec::new();
    for row in -radius..=radius {
        for column in -radius..=radius {
            for (index, (x, y, cols, rows)) in block(column, row).into_iter().enumerate() {
                let rect = Rectangle {
                    x: (column * BLOCK_COLS + x) as f32 * PITCH,
                    y: (row * BLOCK_ROWS + y) as f32 * PITCH,
                    width: cols as f32 * PITCH - GAP,
                    height: rows as f32 * PITCH - GAP,
                };
                let center = rect.center();
                let score = center.x.abs() + center.y.abs() * RANK_VERTICAL_WEIGHT
                    - ((cols * rows) as f32).sqrt() * PITCH * RANK_AREA_WEIGHT;
                slots.push((score, rect, SlotId { column: i64::from(column), row: i64::from(row), index, lot: false }));
            }
        }
    }
    slots.sort_by(|a, b| a.0.total_cmp(&b.0));
    slots.into_iter().take(count).enumerate().map(|(track, (_, rect, slot))| Placement { rect, track, slot }).collect()
}

/// The block's template, picked and mirrored as the original `getBlockOrientation` does.
fn block(column: i32, row: i32) -> [(i32, i32, i32, i32); SLOTS_PER_BLOCK] {
    let (template, reflection) = orientation(column, row);
    reflect(TEMPLATES[template], reflection)
}

/// `getBlockOrientation`: the template steps +1 across and +2 down; the reflection comes from a hash.
fn orientation(column: i32, row: i32) -> (usize, u32) {
    ((column + row * 2).rem_euclid(TEMPLATES.len() as i32) as usize, mix(column, row) % 4)
}

fn reflect(cells: [(i32, i32, i32, i32); SLOTS_PER_BLOCK], reflection: u32) -> [(i32, i32, i32, i32); SLOTS_PER_BLOCK] {
    cells.map(|(x, y, cols, rows)| {
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

// Focus card wiring: the geometry the focus card asks of the wall, and the camera it moves.
impl Wall {
    /// A slot's block at rest and with it grown (`grownBlock`): the block template's reflow, or the lot's cut.
    fn block_shape(&self, slot: SlotId) -> Option<BlockShape> {
        if slot.lot {
            let Layout::Explore(lots) = &self.layout else { return None };
            let lot = lots.get(lots::lot_at(slot.column, slot.row, lots.len()))?;
            if slot.index >= lot.seats.len() {
                return None;
            }
            let origin = Point::new(slot.column as f32 * LOT_PITCH_X, slot.row as f32 * LOT_PITCH_Y);
            let rect = |(x, y, cols, rows): lots::Seat| Rectangle {
                x: origin.x + x as f32 * PITCH,
                y: origin.y + y as f32 * PITCH,
                width: cols as f32 * PITCH - GAP,
                height: rows as f32 * PITCH - GAP,
            };
            let mut base: Vec<Rectangle> = lot.seats.iter().copied().map(rect).collect();
            let mut grown: Vec<Rectangle> = lots::cut_grown(lot, slot.index).into_iter().map(rect).collect();
            // The heading keeps its seat.
            base.push(rect(lot.heading));
            grown.push(rect(lot.heading));
            return Some(BlockShape { base, grown });
        }
        if slot.index >= SLOTS_PER_BLOCK {
            return None;
        }
        let (column, row) = (i32::try_from(slot.column).ok()?, i32::try_from(slot.row).ok()?);
        let (template, reflection) = orientation(column, row);
        let to_rect = |(x, y, cols, rows): (i32, i32, i32, i32)| Rectangle {
            x: (column * BLOCK_COLS + x) as f32 * PITCH,
            y: (row * BLOCK_ROWS + y) as f32 * PITCH,
            width: cols as f32 * PITCH - GAP,
            height: rows as f32 * PITCH - GAP,
        };
        let base = block(column, row).map(to_rect).to_vec();
        let grown =
            reflect(crate::block_reflows::BLOCK_REFLOWS[template][slot.index], reflection).map(to_rect).to_vec();
        Some(BlockShape { base, grown })
    }

    /// `expand`: grows the slot's tile and moves the camera the least that shows the grown card.
    pub fn expand(&mut self, slot: SlotId, tile: usize, bottom_inset: f32) {
        let Some(shape) = self.block_shape(slot) else { return };
        let now = Instant::now();
        let previous = self.focus.expanded().and_then(|(old, _)| self.block_shape(old));
        let target = shape.grown.get(slot.index).copied();
        self.focus.expand(slot, tile, shape, previous, now);
        if let Some(rect) = target {
            self.reveal(rect, bottom_inset, now);
        }
        self.refresh();
    }

    /// `collapse`.
    pub fn collapse(&mut self) {
        let Some((slot, _)) = self.focus.expanded() else { return };
        let base = self.block_shape(slot).map(|shape| shape.base).unwrap_or_default();
        self.focus.collapse(base, Instant::now());
        self.refresh();
    }

    /// `focusSlot`: moves the keyboard focus, revealing where the slot is drawn when asked.
    pub fn focus_slot(&mut self, slot: Option<SlotId>, reveal: bool, bottom_inset: f32) {
        let now = Instant::now();
        self.focus.set_keyboard(slot, now);
        if let (true, Some(slot)) = (reveal, slot)
            && let Some(rest) = self.rest_rect(slot)
        {
            let rect = self.focus.rect(slot, rest, now);
            self.reveal(rect, bottom_inset, now);
        }
    }

    fn reveal(&mut self, rect: Rectangle, bottom_inset: f32, now: Instant) {
        if self.viewport.width < 1.0 {
            return;
        }
        let scale = wall_scale(self.viewport.width);
        let seam = self.seam().width;
        let view = focus_card::View {
            center: self.pan,
            size: self.viewport,
            scale,
            anchor: if self.horizontal { self.anchor_y } else { self.anchor_x },
            seam_width: seam,
            push: seam_push(seam, scale),
            horizontal: self.horizontal,
            seam_edge: SEAM_EDGE,
            bottom_inset: bottom_inset.max(focus_card::REVEAL_PAD),
        };
        let deg = self.tilt.deg(now);
        let target = if deg.abs() > 0.001 && !self.horizontal {
            self.reveal_tilted(rect, view.bottom_inset, deg)
        } else {
            focus_card::reveal(rect, view)
        };
        self.focus.tween_to(self.pan, target, now);
    }

    /// Where a shown slot rests on the wall.
    fn rest_rect(&self, slot: SlotId) -> Option<Rectangle> {
        let finite: &[Placement] = match &self.layout {
            Layout::Finite(placements) => placements,
            _ => &[],
        };
        self.shown.iter().chain(finite).find(|placement| placement.slot == slot).map(|placement| placement.rect)
    }

    /// One frame of the focus card: the camera tween moves the wall; finished folds are dropped.
    pub fn tick_focus(&mut self, now: Instant) -> bool {
        let moved = self.focus.camera(now);
        if let Some(pan) = moved {
            self.pan = pan;
        }
        self.focus.settle(now);
        self.refresh();
        moved.is_some()
    }

    /// The tile and slot under keyboard focus, or the seed: the shown slot nearest the middle of the window.
    pub fn keyboard_tile(&self) -> Option<(usize, SlotId)> {
        let slot = self.focus.keyboard()?;
        self.shown.iter().find(|placement| placement.slot == slot).map(|placement| (placement.track, slot))
    }

    /// `move` on the wall: the adjacent shown slot holding a tile, or the slot nearest the window's middle.
    pub fn neighbour(&self, direction: focus_card::Direction, tiles: &[Tile]) -> Option<SlotId> {
        let now = Instant::now();
        let filled = |placement: &&Placement| tiles.get(placement.track).is_some_and(|tile| !tile.heading);
        let drawn = |placement: &Placement| self.focus.rect(placement.slot, placement.rect, now);
        match self.focus.keyboard().and_then(|slot| self.shown.iter().find(|placement| placement.slot == slot)) {
            Some(from) => focus_card::adjacent(
                drawn(from),
                direction,
                self.shown
                    .iter()
                    .filter(filled)
                    .filter(|placement| placement.slot != from.slot)
                    .map(|placement| (placement.slot, drawn(placement))),
            ),
            None => self
                .shown
                .iter()
                .filter(filled)
                .min_by(|a, b| {
                    let center = Point::new(self.pan.x, self.pan.y);
                    a.rect.center().distance(center).total_cmp(&b.rect.center().distance(center))
                })
                .map(|placement| placement.slot),
        }
    }

    /// The grown card on screen, for the card's body drawn over the wall.
    pub fn expanded_screen(&self, now: Instant) -> Option<(usize, Rectangle, f32)> {
        let (slot, tile) = self.focus.expanded()?;
        let rest = self.rest_rect(slot)?;
        Some((tile, self.screen(self.focus.rect(slot, rest, now)), wall_scale(self.viewport.width)))
    }

    /// Where a shown slot's tile is drawn on screen now.
    pub fn slot_screen(&self, slot: SlotId) -> Option<Rectangle> {
        let placement = self.shown.iter().find(|placement| placement.slot == slot)?;
        Some(self.drawn(placement, Instant::now()))
    }

    /// A tile's cover once it has arrived.
    pub fn cover(&self, id: i64) -> Option<Handle> {
        self.covers.peek(&id).and_then(Option::as_ref).cloned()
    }

    /// `page`: 85% of the window up or down.
    pub fn page(&mut self, direction: i8) {
        if self.viewport.width < 1.0 {
            return;
        }
        let scale = wall_scale(self.viewport.width);
        let target = Vector::new(self.pan.x, self.pan.y + f32::from(direction) * self.viewport.height * 0.85 / scale);
        self.focus.tween_to(self.pan, target, Instant::now());
    }

    /// A drawn tile's rect on screen: moved with its block, popped by keyboard focus.
    fn drawn(&self, placement: &Placement, now: Instant) -> Rectangle {
        let rect = self.screen(self.focus.rect(placement.slot, placement.rect, now));
        let pop = self.focus.pop(placement.slot, now);
        if pop > 0.0 && self.focus.expanded().is_none_or(|(slot, _)| slot != placement.slot) {
            focus_card::popped(rect, GAP * wall_scale(self.viewport.width), pop)
        } else {
            rect
        }
    }
}

/// `.is-expanded .lattice-poster-shade`: a band up from the bottom and one in from the left, over the card's own shade.
fn draw_grown_shade(frame: &mut Frame, rect: Rectangle, palette: &Palette) {
    let Rectangle { x, y, width, height } = rect;
    let mut up = gradient::Linear::new(Point::new(x, y + height), Point::new(x, y));
    for (at, alpha) in focus_card::EXPANDED_SHADE {
        up = up.add_stop(at, palette.shade(alpha));
    }
    frame.fill(&Path::rectangle(rect.position(), rect.size()), up);
    let mut across = gradient::Linear::new(Point::new(x, y), Point::new(x + width, y));
    for (at, alpha) in focus_card::EXPANDED_SIDE {
        across = across.add_stop(at, palette.shade(alpha));
    }
    frame.fill(&Path::rectangle(rect.position(), rect.size()), across);
}

/// Keyboard focus: the top highlight (`::after`, white 10% to nothing at 60%) and, on a tile without the playing
/// stroke, the ring of hairline, ink and hairline outside it.
fn draw_focus(frame: &mut Frame, rect: Rectangle, current: bool, daylight: bool, palette: &Palette) {
    let Rectangle { x, y, height, .. } = rect;
    let glow = gradient::Linear::new(Point::new(x, y), Point::new(x, y + height))
        .add_stop(0.0, Color::from_rgba(1.0, 1.0, 1.0, 0.1))
        .add_stop(0.6, Color::TRANSPARENT);
    frame.fill(&Path::rectangle(rect.position(), rect.size()), glow);
    if current {
        return;
    }
    let mut inner = 0.0;
    for (outer, color) in focus_card::ring(daylight, palette.ink) {
        let middle = (inner + outer) / 2.0;
        let band = Rectangle {
            x: x - middle,
            y: y - middle,
            width: rect.width + 2.0 * middle,
            height: rect.height + 2.0 * middle,
        };
        frame.stroke(
            &Path::rectangle(band.position(), band.size()),
            Stroke::default().with_width(outer - inner).with_color(color),
        );
        inner = outer;
    }
}

impl Wall {
    /// How many tiles a finite (filtered) wall holds; an endless wall has no last tile.
    pub fn finite_count(&self) -> Option<usize> {
        match &self.layout {
            Layout::Finite(placements) => Some(placements.len()),
            _ => None,
        }
    }

    /// The slot showing tile `index` nearest the camera.
    pub fn slot_of(&self, index: usize) -> Option<SlotId> {
        let near = Point::new(self.pan.x, self.pan.y);
        let candidates: &[Placement] = match &self.layout {
            Layout::Finite(placements) => placements,
            _ => &self.shown,
        };
        candidates
            .iter()
            .filter(|placement| placement.track == index)
            .min_by(|a, b| a.rect.center().distance(near).total_cmp(&b.rect.center().distance(near)))
            .map(|placement| placement.slot)
    }
}

impl Wall {
    /// The album wall's art for a drawn tile: the mosaic, or a shard unless the tile is focused or playing.
    fn art<'a>(&'a self, placement: &Placement, item: &Tile, drawn: Rectangle, restored: bool) -> Art<'a> {
        let Some(album) = self.album.as_ref().filter(|album| album.mosaic_on) else { return Art::None };
        let scale = wall_scale(self.viewport.width);
        if let Some(image) = album.mosaic.as_ref().or(album.stand_in.as_ref())
            && !placement.slot.lot
        {
            let block = Size::new(BLOCK_COLS as f32 * PITCH - GAP, BLOCK_ROWS as f32 * PITCH - GAP);
            let pitch = Size::new(BLOCK_COLS as f32 * PITCH, BLOCK_ROWS as f32 * PITCH);
            let world = crate::album_wall::mosaic_square(placement.slot.column, placement.slot.row, block, pitch);
            // The block's cover follows the tile: same offset from the tile on screen as on the wall.
            let rest = self.focus.rect(placement.slot, placement.rect, Instant::now());
            let square = Rectangle {
                x: drawn.x + (world.x - rest.x) * scale,
                y: drawn.y + (world.y - rest.y) * scale,
                width: world.width * scale,
                height: world.height * scale,
            };
            return Art::Mosaic { image, square };
        }
        match album.shards.get(&item.id).filter(|_| !restored) {
            Some(shard) => {
                let place = crate::album_wall::shard_placement(&shard.spots, &shard.seed, drawn.size());
                let rect = Rectangle { x: drawn.x + place.x, y: drawn.y + place.y, ..place };
                Art::Shard { image: &shard.duotone, rect, dots: album.dots.as_ref() }
            }
            None => Art::None,
        }
    }
}

impl Wall {
    /// Where the tilt turns about: the poster's right edge, half way down the window.
    fn pivot(&self) -> Point {
        let seam = self.seam();
        Point::new(seam.center + seam.width / 2.0, self.viewport.height / 2.0)
    }

    /// A point on the window as it stands on the untilted wall.
    fn untilt(&self, point: Point, now: Instant) -> Point {
        let deg = self.tilt.deg(now);
        if deg.abs() <= 0.001 {
            return point;
        }
        let pivot = self.pivot();
        let (sin, cos) = (-deg.to_radians()).sin_cos();
        let (dx, dy) = (point.x - pivot.x, point.y - pivot.y);
        Point::new(pivot.x + dx * cos - dy * sin, pivot.y + dx * sin + dy * cos)
    }

    /// The seam's poster ground in the turned frame, with the paper running on above it and the block below
    /// (`--bravais-poster-bleed: 40vh`) so the turned corners stay covered.
    fn draw_band(&self, frame: &mut Frame, band: crate::poster::Band<'_>) {
        let seam = self.seam();
        let height = self.viewport.height;
        let bleed = height * 0.4;
        let left = seam.center - seam.width / 2.0;
        frame.fill(&Path::rectangle(Point::new(left, -bleed), Size::new(seam.width, bleed)), band.palette.paper);
        frame.fill(&Path::rectangle(Point::new(left, height), Size::new(seam.width, bleed)), band.palette.block);
        frame.with_save(|frame| {
            frame.translate(Vector::new(left, 0.0));
            crate::poster::draw_ground(frame, Size::new(seam.width, height), band);
        });
    }

    /// Whether the layer is turned or turning.
    pub fn tilted(&self, now: Instant) -> bool {
        self.tilt.deg(now).abs() > 0.001
    }

    /// `resolvePosterRevealCenter`: with the layer turned, the camera moves until the turned tile's box fits,
    /// a few rounds since the turn's pivot follows the seam.
    fn reveal_tilted(&self, rect: Rectangle, bottom_inset: f32, deg: f32) -> Vector {
        let scale = wall_scale(self.viewport.width);
        let (sin, cos) = deg.to_radians().sin_cos();
        let pad = focus_card::REVEAL_PAD;
        let shift_into = |start: f32, end: f32, low: f32, high: f32| {
            if end - start > high - low {
                (low + high) / 2.0 - (start + end) / 2.0
            } else if start < low {
                low - start
            } else if end > high {
                high - end
            } else {
                0.0
            }
        };
        let mut center = self.pan;
        for _ in 0..8 {
            let seam = seam_on_axis(
                self.anchor_x,
                center.x,
                self.seam_open,
                self.viewport.width,
                self.viewport.width / 2.0,
                scale,
                false,
            );
            let pivot = Point::new(seam.center + seam.width / 2.0, self.viewport.height / 2.0);
            let push = seam_push(seam.width, scale);
            let mut screen = to_screen(rect, center, self.viewport, scale);
            screen.x += if rect.center_x() < self.anchor_x { -push } else { push };
            let corners = [
                Point::new(screen.x, screen.y),
                Point::new(screen.x + screen.width, screen.y),
                Point::new(screen.x, screen.y + screen.height),
                Point::new(screen.x + screen.width, screen.y + screen.height),
            ]
            .map(|corner| {
                let (dx, dy) = (corner.x - pivot.x, corner.y - pivot.y);
                Point::new(pivot.x + dx * cos - dy * sin, pivot.y + dx * sin + dy * cos)
            });
            let (left, right) = corners.iter().fold((f32::MAX, f32::MIN), |(l, r), c| (l.min(c.x), r.max(c.x)));
            let (top, bottom) = corners.iter().fold((f32::MAX, f32::MIN), |(t, b), c| (t.min(c.y), b.max(c.y)));
            let shift_x = shift_into(left, right, pad, self.viewport.width - pad);
            let shift_y = shift_into(top, bottom, pad, self.viewport.height - bottom_inset);
            if shift_x.abs() < 0.5 && shift_y.abs() < 0.5 {
                break;
            }
            center = Vector::new(center.x - shift_x / scale, center.y - shift_y / (scale * cos.max(0.1)));
        }
        center
    }
}
