// Home page: the cover wall of the current tab (online playlists, radio, explore, albums, or local songs) and its layers.

use std::path::PathBuf;

use folia_core::{Theme, Track};
use folia_i18n::I18n;
use folia_library::Library;
use folia_omni::{Collection, CollectionKind, ExploreEntry, Omni, Shelf};
use folia_render::{Glow, Veil, Wash};
use folia_store::HomeTab;
use std::time::Instant;

use iced::widget::{canvas, column, container, shader, stack, text};
use iced::{Alignment, Color, Element, Length, Task};

use crate::background::{blocking, error_chain};
use crate::lots;
use crate::online::{self, AccountState, Feed, Online};
use crate::wall::{self, CoverSource, Shape, Tile, Wall};

/// `resolveHomeTabs`: playlists, radio, explore, albums, local.
pub const TABS: [HomeTab; 5] = [HomeTab::Playlist, HomeTab::Radio, HomeTab::Explore, HomeTab::Albums, HomeTab::Local];

pub fn tab_label(tab: HomeTab) -> &'static str {
    match tab {
        HomeTab::Playlist => "home.playlists",
        HomeTab::Radio => "home.radio",
        HomeTab::Explore => "home.explore",
        HomeTab::Albums => "home.albums",
        HomeTab::Local => "localMusic.folder",
    }
}

/// What pressing a tile does.
#[derive(Debug, Clone)]
enum Action {
    /// A local song: the whole library is the queue.
    Local(usize),
    Open(Collection),
    /// Personal FM plays straight away (`isPersonalFmCard`).
    Fm(Collection),
    /// An explore song: its section's songs are the queue.
    Song {
        section: usize,
        song: usize,
    },
    /// A song of the open collection.
    LayerTrack(usize),
    None,
}

/// A collection opened from the wall: its songs replace the tiles until the seam's back is pressed.
pub struct Layer {
    pub collection: Collection,
    pub tracks: Feed<Vec<Track>>,
    /// Whether the account saved it; `None` while unknown or where saving does not apply.
    pub subscribed: Option<bool>,
    owner: Option<String>,
    pub subscribing: bool,
    /// The page filter (`useBravaisCollectionFilter`) and the songs it keeps, by index into `tracks`.
    pub query: String,
    matches: Option<Vec<usize>>,
    /// The song list panel is open (a navigation state of the seam).
    pub list: bool,
    /// How far the list is scrolled, for drawing only the rows in view.
    pub list_offset: f32,
    /// The ⋯ more menu is open.
    pub menu: bool,
    pub list_hover: Option<usize>,
    /// The album poster's cover (512px), colours from the cover, release time and composition.
    pub poster: Option<Poster>,
    /// 「过滤当前页」 was pressed on the poster: the seam shows the filter instead (`openBravaisFilter`).
    pub filter_open: bool,
}

/// What the album poster draws besides the collection itself.
pub struct Poster {
    pub cover: Option<iced::widget::image::Handle>,
    pub palette: Option<crate::poster::Palette>,
    pub published: Option<i64>,
    pub composition: crate::poster::Composition,
    /// The ground drawn once for a size and palette.
    pub ground: iced::widget::canvas::Cache,
    /// When the poster last came on screen: its entry plays from here (every mount, as the original's).
    pub entered: Option<Instant>,
    /// The cover flew in from the wall.
    pub flown: bool,
    /// The watermark: the hero word's last character, else the title's.
    pub ghost: String,
    /// The colours were read (or could not be); the shards wait for them.
    pub palette_read: bool,
    /// The album wall's art was asked for.
    art_asked: bool,
    /// The title wrapped for the last column width and size it was asked for.
    title: std::cell::RefCell<Option<std::rc::Rc<crate::poster_motion::TitleLayout>>>,
}

impl Layer {
    fn new(collection: Collection, tracks: Feed<Vec<Track>>) -> Self {
        // The layer key the composition is drawn from (`resolveAlbumComposition(layer.key)`).
        let poster = (collection.kind == CollectionKind::Album).then(|| Poster {
            cover: None,
            palette: None,
            published: None,
            composition: crate::poster::composition(&format!("collection:{}:{}", collection.provider, collection.id)),
            ground: iced::widget::canvas::Cache::new(),
            entered: None,
            title: std::cell::RefCell::new(None),
            flown: false,
            ghost: {
                let words = crate::poster::hero_split(&collection.name);
                let hero = words.iter().find(|(_, hero)| *hero).map(|(text, _)| text.as_str());
                hero.unwrap_or(&collection.name).chars().last().map(String::from).unwrap_or_default()
            },
            // An album with no cover has no colours to wait for.
            palette_read: collection.cover_url.is_none(),
            art_asked: false,
        });
        Self {
            collection,
            tracks,
            subscribed: None,
            owner: None,
            subscribing: false,
            query: String::new(),
            matches: None,
            list: false,
            list_offset: 0.0,
            menu: false,
            list_hover: None,
            poster,
            filter_open: false,
        }
    }

    /// `poster && level === 'full' && !filterOpen`: the list panel and the filter take the poster's place.
    pub fn shows_poster(&self) -> bool {
        self.poster.is_some() && !self.list && !self.filter_open
    }

    /// `contextTracks`: the songs the filter keeps, or all of them, by index into `tracks`.
    pub fn context(&self) -> Vec<usize> {
        let count = self.tracks.ready().map_or(0, Vec::len);
        self.matches.clone().unwrap_or_else(|| (0..count).collect())
    }

    fn context_tracks(&self) -> Vec<Track> {
        let Some(tracks) = self.tracks.ready() else { return Vec::new() };
        self.context().into_iter().filter_map(|index| tracks.get(index).cloned()).collect()
    }

    /// `matchTrackIndexes`: the title, the artists and the album hold the lower-cased query.
    fn refilter(&mut self) {
        let needle = self.query.trim().to_lowercase();
        let Some(tracks) = self.tracks.ready().filter(|_| !needle.is_empty()) else {
            self.matches = None;
            return;
        };
        let text = |track: &Track| {
            let artists = track.artist.clone().unwrap_or_default();
            format!("{} {} {} {}", track.title, artists, track.album.clone().unwrap_or_default(), artists)
                .to_lowercase()
        };
        self.matches = Some(
            tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| text(track).contains(&needle))
                .map(|(index, _)| index)
                .collect(),
        );
    }

    pub fn filtering(&self) -> bool {
        self.matches.is_some()
    }
}

/// What the focus card shows of the player.
#[derive(Debug, Clone, Copy)]
pub struct CardLook {
    pub queued: bool,
    pub playing: bool,
    pub window_width: f32,
}

/// What the shell does after a page message.
pub enum Effect {
    Play {
        queue: Vec<Track>,
        index: usize,
        origin: Option<String>,
    },
    /// The focus card's play on the playing song: pause or resume.
    Toggle,
    /// The focus card's 「进入」: the playing page.
    EnterPlayer,
    /// The focus card's queue button: added, or moved to the end when queued (`applyQueueAddBehavior`).
    EnqueueOne(Track),
    /// Personal FM's first batch; `fm` is the feed later batches come from.
    PlayFm {
        queue: Vec<Track>,
        fm: Collection,
        origin: String,
    },
    Enqueue(Vec<Track>),
    SaveTab(HomeTab),
}

pub struct LibraryPage {
    db_path: PathBuf,
    pub tracks: Vec<Track>,
    pub busy: bool,
    error: Option<String>,
    wall: Wall,
    hint_at: Option<Instant>,
    pub tab: HomeTab,
    pub online: Online,
    pub layer: Option<Layer>,
    tiles: Vec<Tile>,
    actions: Vec<Action>,
    /// Where the playing queue was started, for 「定位」 (`playingSongLocator`).
    source: Option<Source>,
    /// The song to centre on once the collection being opened for 「定位」 has its tracks.
    locating: Option<i64>,
    /// The player bar's safe area at the bottom, which a revealed card stays above.
    pub bottom_inset: f32,
    /// The song playing, for the focus card's play turning into pause / resume.
    pub playing_id: Option<i64>,
    /// The tile last activated, where an album's cover flies out from.
    pressed: Option<(usize, crate::focus_card::SlotId)>,
    /// The poster's cover canvas on screen, and the cover in flight.
    poster_cover: Option<iced::Rectangle>,
    flight: Option<crate::cover_flight::Flight>,
    /// The flight back lands on the tile of this collection.
    flight_home: Option<i64>,
}

/// The wall a queue was started from: a tab's own wall or a collection's.
#[derive(Debug, Clone)]
enum Source {
    Tab(HomeTab),
    Collection(Collection),
}

#[derive(Debug, Clone)]
pub enum Message {
    Import,
    Picked(Option<PathBuf>),
    Reload,
    RefreshExplore,
    /// 「定位」: back to the wall the queue came from, centred on the song with this id.
    Locate(i64),
    Loaded(Result<Vec<Track>, String>),
    Wall(wall::Message),
    Frame,
    Tab(HomeTab),
    Online(online::Message),
    Opened(String, Result<folia_omni::CollectionDetail, String>),
    /// The album poster's cover at 512px, and the colours read from it at 128px.
    PosterCover(String, Option<iced::widget::image::Handle>),
    PosterPalette(String, Option<crate::poster::Palette>),
    /// 「过滤当前页」 on the poster.
    OpenFilter,
    /// Enter in the filter: editing ends and the wall takes the keyboard focus (`focusFirst`).
    FilterSubmit,
    /// A button on the focus card.
    Card(crate::focus_card::Action),
    /// A key on the wall (`resolveBravaisKey`).
    Key(crate::focus_card::Key),
    /// Where the poster's cover canvas is on screen.
    PosterCoverAt(iced::Rectangle),
    /// The album wall's mosaic cover, and one cover baked into a shard with its spots.
    AlbumMosaic(String, Option<iced::widget::image::Handle>),
    AlbumShard(String, String, Option<(iced::widget::image::Handle, Vec<(f32, f32)>)>),
    Back,
    PlayAll,
    EnqueueAll,
    Query(String),
    OpenList,
    CloseList,
    ListScrolled(f32),
    /// A list row: click flies the camera to the song, double-click plays it.
    LocateRow(usize),
    PlayRow(usize),
    ToggleSubscribe,
    Subscribed(String, bool, Result<(), String>),
    CollectionMenu,
    ReloadCollection,
    RowHover(Option<usize>),
}

impl LibraryPage {
    /// Shows what the index already holds; scanning only happens on import or reload.
    pub fn new(db_path: PathBuf, tab: HomeTab, omni: Option<Omni>) -> (Self, Task<Message>) {
        let task = load(db_path.clone(), None, false);
        let (online, restore) = Online::new(omni);
        let page = Self {
            db_path,
            tracks: Vec::new(),
            busy: true,
            error: None,
            wall: Wall::new(),
            hint_at: None,
            tab,
            online,
            layer: None,
            source: None,
            locating: None,
            bottom_inset: 0.0,
            playing_id: None,
            pressed: None,
            poster_cover: None,
            flight: None,
            flight_home: None,
            tiles: Vec::new(),
            actions: Vec::new(),
        };
        (page, Task::batch([task, restore.map(Message::Online)]))
    }

    /// The seam the wall opens for the home chrome, and where it is on screen.
    pub fn set_seam(&mut self, width: f32) {
        self.wall.set_seam(width);
    }

    /// The seam the wall measured. A canvas that has not reported a size yet is closed.
    pub fn seam(&self) -> crate::wall::Seam {
        self.wall.seam()
    }

    pub fn set_horizontal(&mut self, horizontal: bool) {
        self.wall.set_horizontal(horizontal);
    }

    pub fn replan(&mut self, open: f32) {
        self.wall.replan(open);
    }

    pub fn plan_if_needed(&mut self) {
        self.wall.plan_if_needed();
    }

    pub fn coasting(&self) -> bool {
        let now = Instant::now();
        self.wall.coasting()
            || self.wall.focus.animating(now)
            || self.poster_entry(now).0.is_some()
            || self.flight.as_ref().is_some_and(|flight| flight.alive(now))
            || self.wall.tilt.running(now)
    }

    /// The layer is turned with the poster: the seam's own ground and strip give way to the wall's turned band.
    pub fn poster_tilted(&self) -> bool {
        self.wall.tilted(Instant::now())
    }

    /// Where the album poster's entry is.
    fn poster_entry(&self, now: Instant) -> crate::poster_motion::Entry {
        let at = self.layer.as_ref().and_then(|layer| layer.poster.as_ref()).and_then(|poster| poster.entered);
        let ms = at.map(|at| now.saturating_duration_since(at).as_secs_f32() * 1000.0);
        crate::poster_motion::Entry(ms.filter(|ms| *ms < crate::poster_motion::ENTRY_MS))
    }

    /// `activate`: a song grows into the focus card (a second click on it does nothing); anything else opens.
    fn activate(
        &mut self,
        index: usize,
        slot: crate::focus_card::SlotId,
        i18n: &I18n,
    ) -> (Task<Message>, Option<Effect>) {
        self.wall.focus_slot(Some(slot), false, self.bottom_inset);
        self.pressed = Some((index, slot));
        if self.is_song(index) {
            if self.wall.focus.expanded().is_none_or(|(open, _)| open != slot) {
                self.wall.expand(slot, index, self.bottom_inset);
            }
            return (self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall), None);
        }
        self.wall.collapse();
        let pressed = self.press(index, i18n);
        self.pressed = None;
        pressed
    }

    fn is_song(&self, index: usize) -> bool {
        matches!(self.actions.get(index), Some(Action::Local(_) | Action::LayerTrack(_) | Action::Song { .. }))
    }

    /// `assignAlbumCoverArt` once the album's songs and colours are in: the mosaic's big cover, or each shard's
    /// cover read for its spots and baked into the poster's duotone.
    fn album_art(&mut self) -> Task<Message> {
        let Some(omni) = self.online.omni().cloned() else { return Task::none() };
        let Some(layer) = self.layer.as_mut() else { return Task::none() };
        let Some(tracks) = layer.tracks.ready() else { return Task::none() };
        let Some(poster) = layer.poster.as_mut().filter(|poster| poster.palette_read && !poster.art_asked) else {
            return Task::none();
        };
        poster.art_asked = true;
        let palette = poster.palette.unwrap_or(crate::poster::FALLBACK);
        let id = layer.collection.id.clone();
        let mut album = crate::album_wall::AlbumWall { stand_in: poster.cover.clone(), ..Default::default() };
        let task = match crate::album_wall::assign(tracks, layer.collection.cover_url.as_deref()) {
            crate::album_wall::Assignment::Mosaic(url) => {
                // `BRAVAIS_ALBUM_MOSAIC_PX` drawn at the wall's scale lands on the largest cover step.
                Task::perform(crate::background::online_cover(omni, url, 1024), move |cover| {
                    let image = cover
                        .map(|cover| iced::widget::image::Handle::from_rgba(cover.width, cover.height, cover.rgba));
                    Message::AlbumMosaic(id.clone(), image)
                })
            }
            crate::album_wall::Assignment::Shards(urls) => {
                album.dots = Some(crate::album_wall::dots());
                let mut wanted: Vec<String> = urls.into_iter().flatten().collect();
                wanted.sort();
                wanted.dedup();
                Task::batch(wanted.into_iter().map(|url| {
                    let (omni, id, key) = (omni.clone(), id.clone(), url.clone());
                    Task::perform(
                        async move {
                            let sample =
                                crate::background::online_cover_sized(omni.clone(), url.clone(), 128, 50).await;
                            let spots = sample
                                .map(|sample| crate::album_wall::find_spots(&sample.rgba, sample.width as usize))
                                .unwrap_or_default();
                            let cover = crate::background::online_cover(omni, url, 512).await?;
                            let pixels = crate::album_wall::duotone(&cover.rgba, palette.accent, palette.block);
                            Some((iced::widget::image::Handle::from_rgba(cover.width, cover.height, pixels), spots))
                        },
                        move |art| Message::AlbumShard(id.clone(), key.clone(), art),
                    )
                }))
            }
            crate::album_wall::Assignment::Plain => Task::none(),
        };
        self.wall.album = Some(album);
        task
    }

    /// The poster's cover as the flight sees it: the canvas less its margin, turned by the composition.
    fn poster_quad(&self) -> Option<crate::cover_flight::Quad> {
        let rect = self.poster_cover?;
        let deg = self.layer.as_ref()?.poster.as_ref()?.composition.cover_deg;
        let side = rect.width - 2.0 * crate::poster::COVER_MARGIN;
        let margin = crate::poster::COVER_MARGIN;
        let center = iced::Point::new(rect.x + margin + side / 2.0, rect.y + margin + side / 2.0);
        (side > 1.0).then_some(crate::cover_flight::Quad {
            center,
            size: iced::Size::new(side, side),
            angle: deg.to_radians(),
        })
    }

    /// Where the flight is headed this frame: the poster's cover, or the collection's tile on the wall.
    fn flight_target(&self) -> Option<crate::cover_flight::Quad> {
        let flight = self.flight.as_ref()?;
        if !flight.back {
            return self.layer.as_ref().filter(|layer| layer.shows_poster()).and(self.poster_quad());
        }
        let id = self.flight_home?;
        let index = self.tiles.iter().position(|tile| tile.id == id)?;
        let rect = self.wall.slot_screen(self.wall.slot_of(index)?)?;
        Some(crate::cover_flight::Quad::of(rect, 0.0))
    }

    /// The flying cover, drawn over the wall and the seam.
    pub fn flight_view(&self, now: Instant) -> Option<Element<'_, Message>> {
        let flight = self.flight.as_ref().filter(|flight| flight.alive(now))?;
        Some(canvas(crate::cover_flight::Overlay { flight, now }).width(Length::Fill).height(Length::Fill).into())
    }

    /// The song a tile shows, for the focus card.
    fn track_of(&self, index: usize) -> Option<&Track> {
        match self.actions.get(index)? {
            Action::Local(at) => self.tracks.get(*at),
            Action::LayerTrack(at) => self.layer.as_ref()?.tracks.ready()?.get(*at),
            Action::Song { section, song } => self
                .online
                .explore
                .ready()?
                .get(*section)?
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    ExploreEntry::Song(track) => Some(track),
                    ExploreEntry::Collection(_) => None,
                })
                .nth(*song),
            _ => None,
        }
    }

    /// The grown card's song.
    pub fn expanded_track(&self) -> Option<&Track> {
        let (_, tile) = self.wall.focus.expanded()?;
        self.track_of(tile)
    }

    fn card_action(&mut self, action: crate::focus_card::Action, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        let Some((_, tile)) = self.wall.focus.expanded() else { return (Task::none(), None) };
        let current = self.playing_id.is_some() && self.playing_id == self.track_of(tile).map(|track| track.id);
        match action {
            crate::focus_card::Action::Play if current => (Task::none(), Some(Effect::Toggle)),
            crate::focus_card::Action::Play => self.press(tile, i18n),
            crate::focus_card::Action::Enter => (Task::none(), Some(Effect::EnterPlayer)),
            crate::focus_card::Action::Enqueue => (Task::none(), self.track_of(tile).cloned().map(Effect::EnqueueOne)),
        }
    }

    /// The filter is being edited: Esc goes to it before the wall.
    pub fn filter_editing(&self) -> bool {
        self.layer.as_ref().is_some_and(|layer| layer.filter_open)
    }

    /// The wall's keys, Esc taking one step of its ladder (`resolveEscapeStep`).
    fn key(&mut self, key: crate::focus_card::Key, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        use crate::focus_card::Key;
        let bottom = self.bottom_inset;
        match key {
            Key::Escape => {
                // The filter's own Esc first (`BravaisSeamFilterField`): a word is cleared, an empty field closes.
                if let Some(layer) = self.layer.as_mut().filter(|layer| layer.filter_open) {
                    if layer.query.is_empty() {
                        layer.filter_open = false;
                        return (Task::none(), None);
                    }
                    return self.update(Message::Query(String::new()), i18n);
                }
                if self.wall.focus.expanded().is_some() {
                    self.wall.collapse();
                } else if self.wall.focus.keyboard().is_some() {
                    self.wall.focus_slot(None, false, bottom);
                } else if self.layer.as_ref().is_some_and(|layer| layer.list) {
                    return self.update(Message::CloseList, i18n);
                } else if self.layer.as_ref().is_some_and(|layer| !layer.query.is_empty()) {
                    return self.update(Message::Query(String::new()), i18n);
                } else if self.layer.is_some() {
                    return self.update(Message::Back, i18n);
                }
                (Task::none(), None)
            }
            Key::Move(direction) => {
                let next = self.wall.neighbour(direction, &self.tiles);
                if next.is_some() {
                    self.wall.focus_slot(next, true, bottom);
                }
                (self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall), None)
            }
            Key::First | Key::Last => {
                // `first` is rank 0 of a finite wall or the start tile of an endless one; only a finite wall has a last.
                let wanted = match key {
                    Key::First => Some(0),
                    _ => self.wall.finite_count().map(|count| count.saturating_sub(1)),
                };
                if let Some(slot) = wanted.and_then(|index| self.wall.slot_of(index)) {
                    self.wall.focus_slot(Some(slot), true, bottom);
                }
                (Task::none(), None)
            }
            Key::Page(direction) => {
                self.wall.page(direction);
                (Task::none(), None)
            }
            Key::Enter | Key::Enqueue => {
                let Some((tile, slot)) = self.wall.keyboard_tile() else { return (Task::none(), None) };
                let song = self.is_song(tile);
                let open = self.wall.focus.expanded().is_some_and(|(expanded, _)| expanded == slot);
                match (key, song) {
                    (Key::Enter, false) => self.activate(tile, slot, i18n),
                    (Key::Enter, true) if open => self.card_action(crate::focus_card::Action::Play, i18n),
                    (Key::Enter, true) => {
                        self.wall.expand(slot, tile, bottom);
                        (self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall), None)
                    }
                    (_, true) => (Task::none(), self.track_of(tile).cloned().map(Effect::EnqueueOne)),
                    _ => (Task::none(), None),
                }
            }
        }
    }

    pub fn hint_busy(&self, now: Instant) -> bool {
        self.hint_at.is_some_and(|at| now.saturating_duration_since(at).as_secs_f64() < 0.5)
    }

    /// Starts the empty-wall hint fade the first time a hint shows.
    pub fn sync_hint(&mut self, i18n: &I18n, now: Instant) {
        if self.hint_text(i18n).is_some() {
            if self.hint_at.is_none() {
                self.hint_at = Some(now);
            }
        } else {
            self.hint_at = None;
        }
    }

    /// Moves the wall so the local song at `index` is in the middle.
    /// 「定位」: shows the wall the queue was started from, opening its collection again if need be, and
    /// centres the camera on the song's copy nearest it (`useBravaisLocate`).
    fn locate(&mut self, track: i64, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        match self.source.clone() {
            Some(Source::Collection(collection)) => {
                let open = self.layer.as_ref().is_some_and(|layer| {
                    layer.collection.id == collection.id && layer.collection.kind == collection.kind
                });
                if open {
                    return (self.center_on_track(track), None);
                }
                self.locating = Some(track);
                (self.open(collection, i18n), None)
            }
            Some(Source::Tab(tab)) => {
                let (shown, effect) = if tab == self.tab && self.layer.is_none() {
                    (Task::none(), None)
                } else {
                    self.update(Message::Tab(tab), i18n)
                };
                (Task::batch([shown, self.center_on_track(track)]), effect)
            }
            None => (Task::none(), None),
        }
    }

    fn center_on_track(&mut self, track: i64) -> Task<Message> {
        let Some(index) = self.tiles.iter().position(|tile| tile.id == track && !tile.heading) else {
            return Task::none();
        };
        self.wall.center_on(index);
        self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall)
    }

    /// Widens the seam into the full info strip while a collection layer is open.
    /// Closes the collection's ⋯ menu, as a press outside it or Escape does.
    pub fn dismiss_menu(&mut self) {
        if let Some(layer) = self.layer.as_mut() {
            layer.menu = false;
        }
    }

    pub fn menu_open(&self) -> bool {
        self.layer.as_ref().is_some_and(|layer| layer.menu)
    }

    pub fn set_tab_chrome(&mut self, home: &mut crate::home::HomeChrome, now: Instant) {
        let list = self.layer.as_ref().is_some_and(|layer| layer.list);
        let poster = self.layer.as_ref().is_some_and(Layer::shows_poster);
        // `has-album-wall`: only while the poster is on screen; the stand-in is the poster's cover once it arrives.
        let stand_in = self.layer.as_ref().and_then(|layer| layer.poster.as_ref()).and_then(|art| art.cover.clone());
        // `useBravaisPosterTilt`: the layer turns while a poster stands in an upright seam.
        let upright = !self.wall.seam().horizontal && self.wall.seam().width > 0.0;
        self.wall.tilt.set(poster && upright, now);
        if let Some(album) = self.wall.album.as_mut() {
            album.mosaic_on = poster;
            if album.stand_in.is_none() {
                album.stand_in = stand_in;
            }
        }
        // The poster mounts: its entry starts; it leaves (list, filter): the next mount plays it again.
        if let Some(art) = self.layer.as_mut().and_then(|layer| layer.poster.as_mut()) {
            match (poster, art.entered) {
                (true, None) => art.entered = Some(now),
                (false, Some(_)) => art.entered = None,
                _ => {}
            }
        }
        home.set_collection(self.layer.is_some(), list, poster, now);
    }

    pub fn poster_ground(&self) -> Option<&iced::widget::canvas::Cache> {
        self.layer
            .as_ref()
            .filter(|layer| layer.shows_poster())
            .and_then(|layer| layer.poster.as_ref())
            .map(|poster| &poster.ground)
    }

    /// After a confirmed sign-in, keeps the session and reads the account.
    pub fn signed_in(&mut self) -> Task<Message> {
        self.online.signed_in_now().map(Message::Online)
    }

    pub fn sign_out(&mut self, i18n: &I18n) -> Task<Message> {
        let task = self.online.sign_out().map(Message::Online);
        self.layer = None;
        Task::batch([task, self.rebuild(i18n)])
    }

    /// Handles a page message; returns what the shell should do next.
    pub fn update(&mut self, message: Message, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        match message {
            Message::Import => {
                let pick =
                    async { rfd::AsyncFileDialog::new().pick_folder().await.map(|folder| folder.path().to_owned()) };
                (Task::perform(pick, Message::Picked), None)
            }
            Message::Picked(Some(folder)) => (self.start(Some(folder), true), None),
            Message::Picked(None) => (Task::none(), None),
            Message::Reload => (self.start(None, true), None),
            Message::RefreshExplore => (self.online.refresh_explore().map(Message::Online), None),
            Message::Locate(track) => self.locate(track, i18n),
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(tracks) => {
                        self.tracks = tracks;
                        self.error = None;
                        if self.tab == HomeTab::Local && self.layer.is_none() {
                            return (self.rebuild(i18n), None);
                        }
                    }
                    Err(err) => self.error = Some(err),
                }
                (Task::none(), None)
            }
            Message::Wall(message) => {
                let (task, pressed) = self.wall.update(message, &self.tiles, self.online.omni());
                let task = task.map(Message::Wall);
                match pressed {
                    Some(Some((index, slot))) => {
                        let (more, effect) = self.activate(index, slot, i18n);
                        (Task::batch([task, more]), effect)
                    }
                    Some(None) => {
                        self.wall.collapse();
                        (task, None)
                    }
                    None => (task, None),
                }
            }
            Message::Card(action) => self.card_action(action, i18n),
            Message::PosterCoverAt(rect) => {
                self.poster_cover = Some(rect);
                (Task::none(), None)
            }
            Message::AlbumMosaic(id, image) => {
                if self.layer.as_ref().is_some_and(|layer| layer.collection.id == id)
                    && let Some(album) = self.wall.album.as_mut()
                {
                    album.mosaic = image;
                }
                (Task::none(), None)
            }
            Message::AlbumShard(id, url, art) => {
                let layer = self.layer.as_ref().filter(|layer| layer.collection.id == id);
                let (Some(layer), Some((duotone, spots))) = (layer, art) else { return (Task::none(), None) };
                let tracks = layer.tracks.ready().map(Vec::as_slice).unwrap_or_default();
                let assigned = crate::album_wall::assign(tracks, layer.collection.cover_url.as_deref());
                if let (crate::album_wall::Assignment::Shards(urls), Some(album)) = (assigned, self.wall.album.as_mut())
                {
                    for (track, wanted) in tracks.iter().zip(urls) {
                        if wanted.as_deref() == Some(url.as_str()) {
                            let seed = match &track.online {
                                Some(online) => format!("online:{}:{}", online.provider, online.id),
                                None => format!("local:{}", track.id),
                            };
                            let shard =
                                crate::album_wall::Shard { duotone: duotone.clone(), spots: spots.clone(), seed };
                            album.shards.insert(track.id, shard);
                        }
                    }
                }
                (Task::none(), None)
            }
            Message::Key(key) => self.key(key, i18n),
            Message::FilterSubmit => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.filter_open = false;
                }
                let first = self.wall.neighbour(crate::focus_card::Direction::Down, &self.tiles);
                self.wall.focus_slot(first, true, self.bottom_inset);
                (Task::none(), None)
            }
            Message::Frame => {
                let now = Instant::now();
                let target = self.flight_target();
                if let Some(flight) = self.flight.as_mut() {
                    flight.step(target, now);
                    if !flight.alive(now) {
                        self.flight = None;
                    }
                }
                if (self.wall.focus.animating(now) || self.wall.focus.expanded().is_some()) && self.wall.tick_focus(now)
                {
                    return (self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall), None);
                }
                (self.wall.tick_coast(now, &self.tiles, self.online.omni()).map(Message::Wall), None)
            }
            Message::Tab(tab) => {
                if tab == self.tab && self.layer.is_none() {
                    return (Task::none(), None);
                }
                if self.layer.is_none() {
                    self.wall.wave_next();
                }
                self.tab = tab;
                self.layer = None;
                let ensure = self.online.ensure(tab).map(Message::Online);
                (Task::batch([ensure, self.rebuild(i18n)]), Some(Effect::SaveTab(tab)))
            }
            Message::Online(message) => {
                let (task, changed) = self.online.update(message);
                let mut tasks = vec![task.map(Message::Online)];
                if changed {
                    tasks.push(self.online.ensure(self.tab).map(Message::Online));
                    if self.tab.is_online() && self.layer.is_none() {
                        tasks.push(self.rebuild(i18n));
                    }
                }
                (Task::batch(tasks), None)
            }
            Message::Opened(id, result) => {
                let Some(layer) = self.layer.as_mut().filter(|layer| layer.collection.id == id) else {
                    return (Task::none(), None);
                };
                layer.tracks = match result {
                    Ok(detail) => {
                        if let Some(poster) = layer.poster.as_mut() {
                            poster.published = detail.published;
                        }
                        layer.subscribed = detail.subscribed;
                        layer.owner = detail.owner;
                        Feed::Ready(detail.tracks)
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "collection unavailable");
                        Feed::Failed(err)
                    }
                };
                layer.refilter();
                let art = self.album_art();
                let rebuilt = Task::batch([self.rebuild(i18n), art]);
                let located = match self.locating.take() {
                    Some(track) => self.center_on_track(track),
                    None => Task::none(),
                };
                (Task::batch([rebuilt, located]), None)
            }
            Message::PosterCover(id, cover) => {
                if let Some(poster) = self
                    .layer
                    .as_mut()
                    .filter(|layer| layer.collection.id == id)
                    .and_then(|layer| layer.poster.as_mut())
                {
                    poster.cover = cover;
                }
                (Task::none(), None)
            }
            Message::PosterPalette(id, palette) => {
                if let Some(poster) = self
                    .layer
                    .as_mut()
                    .filter(|layer| layer.collection.id == id)
                    .and_then(|layer| layer.poster.as_mut())
                {
                    poster.palette = palette;
                    poster.palette_read = true;
                    poster.ground.clear();
                }
                (self.album_art(), None)
            }
            Message::OpenFilter => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.filter_open = true;
                    layer.menu = false;
                }
                (iced::widget::operation::focus(crate::home::filter_id()), None)
            }
            Message::Back => {
                if let Some(layer) = self.layer.as_mut().filter(|layer| layer.list) {
                    // The panel is a navigation state: back closes it first.
                    layer.list = false;
                    return (Task::none(), None);
                }
                // `flyCoverBack`: from the poster's cover back onto the album's tile on the wall.
                let back = self.layer.as_ref().filter(|layer| layer.shows_poster()).and_then(|layer| {
                    let image = layer.poster.as_ref()?.cover.clone()?;
                    Some((
                        image,
                        layer.collection.wall_id(),
                        layer.poster.as_ref()?.palette.unwrap_or(crate::poster::FALLBACK).ink,
                    ))
                });
                let from = self.poster_quad();
                self.layer = None;
                self.wall.album = None;
                self.poster_cover = None;
                if let (Some((image, home, ink)), Some(from)) = (back, from) {
                    self.flight = Some(crate::cover_flight::Flight::new(image, from, true, ink, Instant::now()));
                    self.flight_home = Some(home);
                }
                (self.rebuild(i18n), None)
            }
            Message::PlayAll => {
                let Some(layer) = &self.layer else { return (Task::none(), None) };
                let queue = layer.context_tracks();
                if queue.is_empty() {
                    return (Task::none(), None);
                }
                self.source = Some(Source::Collection(layer.collection.clone()));
                (Task::none(), Some(Effect::Play { queue, index: 0, origin: Some(layer.collection.name.clone()) }))
            }
            Message::EnqueueAll => {
                let Some(layer) = &self.layer else { return (Task::none(), None) };
                let queue = layer.context_tracks();
                ((Task::none()), (!queue.is_empty()).then_some(Effect::Enqueue(queue)))
            }
            Message::Query(query) => {
                let Some(layer) = self.layer.as_mut() else { return (Task::none(), None) };
                let was = layer.filtering();
                layer.query = query;
                layer.refilter();
                layer.list_offset = 0.0;
                if !was && !layer.filtering() {
                    return (Task::none(), None);
                }
                (self.rebuild(i18n), None)
            }
            Message::OpenList | Message::CloseList => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.list = matches!(message, Message::OpenList);
                    layer.menu = false;
                }
                (Task::none(), None)
            }
            Message::ListScrolled(offset) => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.list_offset = offset;
                }
                (Task::none(), None)
            }
            Message::LocateRow(row) => {
                // The list's rows are the wall's tiles in order: the camera flies to the copy nearest it.
                self.wall.center_on(row);
                (self.wall.show_visible(&self.tiles, self.online.omni()).map(Message::Wall), None)
            }
            Message::PlayRow(row) => self.press(row, i18n),
            Message::ToggleSubscribe => {
                let (Some(layer), Some(omni)) = (self.layer.as_mut(), self.online.omni().cloned()) else {
                    return (Task::none(), None);
                };
                let Some(subscribed) = layer.subscribed.filter(|_| !layer.subscribing) else {
                    return (Task::none(), None);
                };
                layer.subscribing = true;
                let (id, wanted, collection) = (layer.collection.id.clone(), !subscribed, layer.collection.clone());
                let work =
                    async move { omni.set_subscribed(&collection, wanted).await.map_err(|err| error_chain(&err)) };
                (Task::perform(work, move |result| Message::Subscribed(id.clone(), wanted, result)), None)
            }
            Message::Subscribed(id, wanted, result) => {
                let Some(layer) = self.layer.as_mut().filter(|layer| layer.collection.id == id) else {
                    return (Task::none(), None);
                };
                layer.subscribing = false;
                match result {
                    Ok(()) => layer.subscribed = Some(wanted),
                    Err(err) => tracing::warn!(error = %err, "collection not saved"),
                }
                (Task::none(), None)
            }
            Message::RowHover(row) => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.list_hover = row;
                }
                (Task::none(), None)
            }
            Message::CollectionMenu => {
                if let Some(layer) = self.layer.as_mut() {
                    layer.menu = !layer.menu;
                }
                (Task::none(), None)
            }
            Message::ReloadCollection => {
                let Some(collection) = self.layer.as_ref().map(|layer| layer.collection.clone()) else {
                    return (Task::none(), None);
                };
                self.layer = None;
                (self.open(collection, i18n), None)
            }
        }
    }

    /// What pressing the tile at `index` does: play from it, or open its collection as a layer.
    fn press(&mut self, index: usize, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        match self.actions.get(index).cloned().unwrap_or(Action::None) {
            Action::Local(index) => {
                self.source = Some(Source::Tab(HomeTab::Local));
                (Task::none(), Some(Effect::Play { queue: self.tracks.clone(), index, origin: None }))
            }
            Action::LayerTrack(index) => {
                // `playTrack`: the song plays with the songs the filter keeps as its queue.
                let Some(layer) = &self.layer else { return (Task::none(), None) };
                let context = layer.context();
                let Some(position) = context.iter().position(|kept| *kept == index) else {
                    return (Task::none(), None);
                };
                let origin = Some(layer.collection.name.clone());
                let queue = layer.context_tracks();
                self.source = Some(Source::Collection(layer.collection.clone()));
                (Task::none(), Some(Effect::Play { queue, index: position, origin }))
            }
            Action::Fm(fm) => {
                let Some(feed) = self.online.radio.ready().filter(|feed| !feed.personal_fm.is_empty()) else {
                    return (Task::none(), None);
                };
                let origin = i18n.t("home.personalFm").to_owned();
                let queue = feed.personal_fm.clone();
                // Personal FM is no wall of its own: there is nowhere to locate its songs.
                self.source = None;
                (Task::none(), Some(Effect::PlayFm { queue, fm, origin }))
            }
            Action::Song { section, song } => {
                let Some(shelf) = self.online.explore.ready().and_then(|sections| sections.get(section)) else {
                    return (Task::none(), None);
                };
                let songs: Vec<Track> = shelf
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        ExploreEntry::Song(track) => Some(track.clone()),
                        ExploreEntry::Collection(_) => None,
                    })
                    .collect();
                let origin = Some(shelf_heading(&shelf.shelf, i18n).to_owned());
                self.source = Some(Source::Tab(HomeTab::Explore));
                (Task::none(), Some(Effect::Play { queue: songs, index: song, origin }))
            }
            Action::Open(collection) => {
                self.wall.flip_from_press();
                (self.open(collection, i18n), None)
            }
            Action::None => (Task::none(), None),
        }
    }

    fn open(&mut self, collection: Collection, i18n: &I18n) -> Task<Message> {
        let daily = (collection.kind == CollectionKind::DailySongs)
            .then(|| self.online.radio.ready().map(|feed| feed.daily_songs.clone()))
            .flatten();
        let fetch = match (&daily, self.online.omni().cloned()) {
            (Some(_), _) => Task::none(),
            (None, Some(omni)) => {
                let (id, wanted) = (collection.id.clone(), collection.clone());
                let work = async move { omni.collection_detail(&wanted).await.map_err(|err| error_chain(&err)) };
                Task::perform(work, move |result| Message::Opened(id.clone(), result))
            }
            (None, None) => Task::none(),
        };
        let tracks = daily.map_or(Feed::Loading, Feed::Ready);
        let art = match (collection.kind, collection.cover_url.clone(), self.online.omni().cloned()) {
            (CollectionKind::Album, Some(url), Some(omni)) => {
                let id = collection.id.clone();
                let (sample_omni, sample_url, sample_id) = (omni.clone(), url.clone(), id.clone());
                // `BRAVAIS_ALBUM_COVER_PX`; the colours are read at `SAMPLE_COVER_PX` drawn 50px wide (`loadImagePixels`).
                let cover = Task::perform(crate::background::online_cover(omni, url, 512), move |cover| {
                    let handle = cover
                        .map(|cover| iced::widget::image::Handle::from_rgba(cover.width, cover.height, cover.rgba));
                    Message::PosterCover(id.clone(), handle)
                });
                let palette = Task::perform(
                    async move {
                        let sample = crate::background::online_cover_sized(sample_omni, sample_url, 128, 50).await?;
                        // `ALBUM_SAMPLE_COLORS`: two for the cover's own pair, five to choose the accent from.
                        let pair = crate::poster::representative(&sample.rgba, 2);
                        let lead = crate::poster::representative(&sample.rgba, 5);
                        let colors: Vec<_> = pair
                            .iter()
                            .copied()
                            .chain(lead.into_iter().filter(|color| !pair.contains(color)))
                            .collect();
                        crate::poster::album_palette(&colors)
                    },
                    move |palette| Message::PosterPalette(sample_id.clone(), palette),
                );
                Task::batch([cover, palette])
            }
            _ => Task::none(),
        };
        let album = collection.kind == CollectionKind::Album;
        let source = self.pressed.take().filter(|_| album).and_then(|(tile, slot)| {
            let rect = self.wall.slot_screen(slot)?;
            let image = self.wall.cover(self.tiles.get(tile)?.id)?;
            Some((rect, image))
        });
        self.layer = Some(Layer::new(collection, tracks));
        self.poster_cover = None;
        self.wall.album = None;
        // `flyCoverToPoster`: from the tile that was clicked to the poster's cover, which hides until it lands.
        if let Some((rect, image)) = source {
            let ink = crate::poster::FALLBACK.ink;
            let now = Instant::now();
            self.flight = Some(crate::cover_flight::Flight::new(
                image,
                crate::cover_flight::Quad::of(rect, 0.0),
                false,
                ink,
                now,
            ));
            if let Some(poster) = self.layer.as_mut().and_then(|layer| layer.poster.as_mut()) {
                poster.flown = true;
            }
        }
        Task::batch([fetch, art, self.rebuild(i18n)])
    }

    /// Builds the tiles of the current layer or tab and lays them on the wall.
    fn rebuild(&mut self, i18n: &I18n) -> Task<Message> {
        let (tiles, actions, shape) = self.compose(i18n);
        self.wall.capture(&self.tiles);
        self.tiles = tiles;
        self.actions = actions;
        self.wall.relayout(&self.tiles, shape, self.online.omni()).map(Message::Wall)
    }

    fn compose(&self, i18n: &I18n) -> (Vec<Tile>, Vec<Action>, Shape) {
        if let Some(layer) = &self.layer {
            let tracks = layer.tracks.ready().map(Vec::as_slice).unwrap_or_default();
            let context = layer.context();
            let tiles = context
                .iter()
                .filter_map(|index| tracks.get(*index))
                .map(|track| track_tile(track, None, i18n))
                .collect();
            // Filtering turns the edgeless wall into a ranked one of the matches (`mode: 'finite'`).
            let shape = if layer.filtering() { Shape::Finite } else { Shape::Blocks };
            return (tiles, context.into_iter().map(Action::LayerTrack).collect(), shape);
        }
        if self.tab == HomeTab::Local {
            let tiles = self.tracks.iter().map(|track| track_tile(track, None, i18n)).collect();
            return (tiles, (0..self.tracks.len()).map(Action::Local).collect(), Shape::Blocks);
        }
        if self.online.signed_in().is_none() {
            return (Vec::new(), Vec::new(), Shape::Blocks);
        }
        let mut tiles = Vec::new();
        let mut actions = Vec::new();
        let card = |collection: &Collection, description: String, tiles: &mut Vec<Tile>, actions: &mut Vec<Action>| {
            tiles.push(card_tile(collection, description, i18n));
            actions.push(if collection.kind == CollectionKind::PersonalFm {
                Action::Fm(collection.clone())
            } else {
                Action::Open(collection.clone())
            });
        };
        match self.tab {
            HomeTab::Playlist => {
                for collection in self.online.playlists.ready().into_iter().flatten() {
                    let description = collection.creator.clone().unwrap_or_else(|| i18n.t("home.playlists").to_owned());
                    card(collection, description, &mut tiles, &mut actions);
                }
            }
            HomeTab::Albums => {
                for collection in self.online.albums.ready().into_iter().flatten() {
                    let description =
                        collection.creator.clone().unwrap_or_else(|| i18n.t("player.unknownArtist").to_owned());
                    card(collection, description, &mut tiles, &mut actions);
                }
            }
            HomeTab::Radio => {
                if let Some(feed) = self.online.radio.ready() {
                    for collection in radio_cards(feed, i18n) {
                        let description = radio_description(&collection, i18n);
                        card(&collection, description, &mut tiles, &mut actions);
                    }
                }
            }
            HomeTab::Explore => return self.compose_explore(i18n),
            HomeTab::Local => {}
        }
        (tiles, actions, Shape::Blocks)
    }

    /// `projectExploreItems` and `projectExploreHeadings`: entries first, then one heading tile per section.
    fn compose_explore(&self, i18n: &I18n) -> (Vec<Tile>, Vec<Action>, Shape) {
        let Some(sections) = self.online.explore.ready() else { return (Vec::new(), Vec::new(), Shape::Blocks) };
        let mut tiles = Vec::new();
        let mut actions = Vec::new();
        let mut laid = Vec::new();
        for (section_index, section) in sections.iter().enumerate() {
            let mut entries = Vec::new();
            let mut song = 0;
            for entry in &section.entries {
                let index = tiles.len();
                match entry {
                    ExploreEntry::Song(track) => {
                        tiles.push(track_tile(track, Some(i18n.t("libraryBravais.kind.track").to_owned()), i18n));
                        actions.push(Action::Song { section: section_index, song });
                        entries.push((index, lots::Rank::Song));
                        song += 1;
                    }
                    ExploreEntry::Collection(collection) => {
                        let description =
                            collection.creator.clone().or_else(|| collection.description.clone()).unwrap_or_default();
                        let mut tile = card_tile(collection, description, i18n);
                        tile.badge = Some(i18n.t(kind_label(collection.kind)).to_owned());
                        tiles.push(tile);
                        actions.push(Action::Open(collection.clone()));
                        let rank = if collection.kind == CollectionKind::Album {
                            lots::Rank::Album
                        } else {
                            lots::Rank::Playlist
                        };
                        entries.push((index, rank));
                    }
                }
            }
            laid.push((section.key.clone(), section_index, entries));
        }
        let count = sections.len();
        let mut shape = Vec::new();
        for (key, section_index, entries) in laid {
            let heading = tiles.len();
            let section = &sections[section_index];
            tiles.push(Tile {
                id: -1 - heading as i64,
                title: shelf_heading(&section.shelf, i18n).to_owned(),
                subtitle: section_summary(section, i18n),
                badge: Some(format!("{:02} / {:02}", section_index + 1, count)),
                cover: CoverSource::None,
                heading: true,
                hue: 0.0,
                stack: false,
            });
            actions.push(Action::None);
            shape.push(lots::Section { key, heading, entries });
        }
        (tiles, actions, Shape::Explore(shape))
    }

    /// The seam's status line (`BravaisHomeDirectory` status).
    pub fn status(&self, i18n: &I18n) -> Option<String> {
        if let Some(layer) = &self.layer {
            // `projectCollectionStatus`: loading, a failed read, an empty collection, or a filter without matches.
            return match &layer.tracks {
                Feed::Loading | Feed::Idle => Some(i18n.t("playlist.loading").to_owned()),
                Feed::Failed(err) => Some(i18n.format("playlist.loadFailed", &[("error", err)])),
                Feed::Ready(tracks) if tracks.is_empty() => Some(i18n.t("home.loadingLibrary").to_owned()),
                Feed::Ready(_) if layer.matches.as_ref().is_some_and(Vec::is_empty) => {
                    Some(i18n.t("home.gridSearchNoResults").to_owned())
                }
                Feed::Ready(_) => None,
            };
        }
        if self.tab == HomeTab::Local {
            if self.error.is_some() {
                return None;
            }
            if self.busy {
                return Some(i18n.t("localMusic.importing").to_owned());
            }
            return self.tracks.is_empty().then(|| i18n.t("localMusic.noLocalMusic").to_owned());
        }
        match &self.online.account {
            AccountState::Guest { ran_out } => ran_out.then(|| i18n.t("status.loginExpired").to_owned()),
            AccountState::Resolving => Some(i18n.t("home.loadingLibrary").to_owned()),
            AccountState::SignedIn(_) if !self.tiles.is_empty() => None,
            AccountState::SignedIn(_) if self.tab == HomeTab::Explore => Some(
                i18n.t(match &self.online.explore {
                    Feed::Idle | Feed::Loading => "playlist.loading",
                    Feed::Failed(_) => "home.exploreFailed",
                    Feed::Ready(_) => "home.exploreEmpty",
                })
                .to_owned(),
            ),
            AccountState::SignedIn(_) => {
                let loading = match self.tab {
                    HomeTab::Playlist => self.online.playlists.loading(),
                    HomeTab::Albums => self.online.albums.loading(),
                    HomeTab::Radio => self.online.radio.loading(),
                    _ => false,
                };
                Some(i18n.t(if loading { "playlist.loading" } else { "home.loadingLibrary" }).to_owned())
            }
        }
    }

    /// What the seam shows of the open collection: title, count, filter, star, the list rows in view and the menu.
    /// `refreshTool`: on the explore tab of a signed-in account, enabled unless a read is on its way.
    pub fn explore_refresh(&self) -> Option<bool> {
        let shown = self.tab == HomeTab::Explore && self.online.signed_in().is_some() && self.layer.is_none();
        shown.then(|| self.online.can_refresh_explore())
    }

    pub fn layer_seam(
        &self,
        i18n: &I18n,
        playing: Option<i64>,
        rows_in_view: usize,
        window: iced::Size,
    ) -> Option<crate::home::LayerSeam> {
        let layer = self.layer.as_ref()?;
        let tracks = layer.tracks.ready().map(Vec::as_slice).unwrap_or_default();
        let context = layer.context();
        let total = tracks.len().max(layer.collection.track_count.unwrap_or(0) as usize);
        let match_label = i18n.format(
            "libraryBravaisCollection.matchCount",
            &[("matches", &context.len().to_string()), ("total", &tracks.len().to_string())],
        );
        let meta = if layer.filtering() { match_label.clone() } else { track_count(i18n, total as u32) };
        let savable = matches!(layer.collection.kind, CollectionKind::Playlist | CollectionKind::Album);
        let own = layer.owner.is_some()
            && layer.owner.as_deref() == self.online.signed_in().map(|account| account.id.as_str());
        let star = layer.subscribed.filter(|_| savable && !own).map(|saved| {
            let key = match (layer.collection.kind == CollectionKind::Album, saved) {
                (true, true) => "options.unsubscribeAlbum",
                (true, false) => "options.subscribeAlbum",
                (false, true) => "options.unsubscribePlaylist",
                (false, false) => "options.subscribePlaylist",
            };
            (saved, layer.subscribing, i18n.t(key).to_owned())
        });
        let first = if layer.list { (layer.list_offset / crate::home::LIST_ROW) as usize } else { 0 };
        let first = first.saturating_sub(6).min(context.len());
        let rows = if layer.list {
            context
                .iter()
                .enumerate()
                .skip(first)
                .take(rows_in_view + 12)
                .filter_map(|(row, index)| tracks.get(*index).map(|track| (row, track)))
                .map(|(row, track)| crate::home::ListRow {
                    badge: format!("{:02}", row + 1),
                    title: track.title.clone(),
                    subtitle: track.artist.clone().unwrap_or_default(),
                    time: if track.duration > 0.0 {
                        crate::background::format_time(track.duration)
                    } else {
                        String::new()
                    },
                    current: playing == Some(track.id),
                })
                .collect()
        } else {
            Vec::new()
        };
        Some(crate::home::LayerSeam {
            title: layer.collection.name.clone(),
            meta,
            query: layer.query.clone(),
            filtering: layer.filtering(),
            match_label,
            star,
            list: layer.list,
            first,
            count: context.len(),
            rows,
            menu: layer.menu,
            playable: !context.is_empty(),
            album: layer.poster.as_ref().filter(|_| layer.shows_poster()).map(|poster| {
                let year = poster.published.map(crate::poster::year_of);
                let kind = i18n.t(kind_label(CollectionKind::Album));
                let words = crate::poster::hero_split(&layer.collection.name);
                let ghost = poster.ghost.clone();
                // The turned poster's safe area: the paper's edge moves this far in at the top and bottom.
                let deg = self.wall.tilt.deg(Instant::now());
                let inset = window.height / 2.0 * deg.abs().to_radians().tan();
                crate::home::AlbumSeam {
                    palette: poster.palette.unwrap_or(crate::poster::FALLBACK),
                    composition: poster.composition,
                    cover: poster.cover.clone(),
                    kicker: year.map_or_else(|| kind.to_owned(), |year| format!("{kind} · {year}")),
                    artists: layer.collection.creator.clone().unwrap_or_default(),
                    // NetEase gives an album no length or genre: its facts are the count alone.
                    facts: (total > 0).then(|| track_count(i18n, total as u32)).into_iter().collect(),
                    description: layer.collection.description.clone().filter(|text| !text.trim().is_empty()),
                    ghost,
                    title: {
                        // `--album-title-size`: min(150px, 15cqh, (100cqw − 2·pad) / (columns + 1.4)).
                        let column = crate::poster::width(window.width) - 2.0 * crate::poster::PAD - 2.0 * inset;
                        let size = 150f32
                            .min(window.height * 0.15)
                            .min(column / (crate::poster::title_columns(&layer.collection.name) + 1.4));
                        let mut kept = poster.title.borrow_mut();
                        match kept.as_ref().filter(|title| title.width == column && title.size == size) {
                            Some(title) => title.clone(),
                            None => {
                                let laid = std::rc::Rc::new(crate::poster_motion::layout_title(
                                    &words,
                                    size,
                                    column,
                                    crate::poster::TITLE_FONT,
                                ));
                                *kept = Some(laid.clone());
                                laid
                            }
                        }
                    },
                    entry: self.poster_entry(Instant::now()),
                    flying: self.flight.as_ref().is_some_and(crate::cover_flight::Flight::pending),
                    flown: poster.flown,
                    inset,
                }
            }),
        })
    }

    /// The empty-wall copy: online tabs without an account (`onlineWallHint`), or an empty local library.
    fn hint_text(&self, i18n: &I18n) -> Option<(String, String)> {
        if self.layer.is_some() {
            return None;
        }
        if self.tab == HomeTab::Local {
            let empty = self.error.is_none() && !self.busy && self.tracks.is_empty();
            let action = i18n.t("localMusic.importFolder");
            let body = i18n.format("libraryBravaisHome.wallHint.localBody", &[("action", action)]);
            return empty.then(|| (i18n.t("libraryBravaisHome.wallHint.localTitle").to_owned(), body));
        }
        let AccountState::Guest { ran_out } = self.online.account else { return None };
        let connect = i18n.t("libraryBravaisHome.connect");
        if ran_out {
            let provider = provider_label(self.online.omni());
            return Some((
                i18n.format("libraryBravaisHome.wallHint.reloginTitle", &[("provider", &provider)]),
                i18n.format("libraryBravaisHome.wallHint.reloginBody", &[("connect", connect)]),
            ));
        }
        let local = i18n.t(tab_label(HomeTab::Local));
        Some((
            i18n.t("libraryBravaisHome.wallHint.guestTitle").to_owned(),
            i18n.format("libraryBravaisHome.wallHint.guestBodyWithLocal", &[("connect", connect), ("local", local)]),
        ))
    }

    fn start(&mut self, folder: Option<PathBuf>, scan: bool) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        load(self.db_path.clone(), folder, scan)
    }

    pub fn view<'a>(
        &'a self,
        i18n: &'a I18n,
        theme: &'a Theme,
        daylight: bool,
        muted: Color,
        playing: Option<i64>,
        look: CardLook,
    ) -> Element<'a, Message> {
        let band =
            self.layer.as_ref().filter(|layer| layer.shows_poster()).and_then(|layer| layer.poster.as_ref()).map(
                |poster| crate::poster::Band {
                    palette: poster.palette.unwrap_or(crate::poster::FALLBACK),
                    composition: poster.composition,
                    ghost: &poster.ghost,
                    entry: self.poster_entry(Instant::now()),
                },
            );
        let wall: Element<'a, wall::Message> = canvas(self.wall.view(&self.tiles, theme, daylight, playing, band))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let mut layers = stack![ground(theme), wall.map(Message::Wall)];
        // The focus card's body over its grown tile.
        if let (Some((tile, rect, scale)), Some(_)) =
            (self.wall.expanded_screen(Instant::now()), self.wall.focus.expanded())
            && let Some(track) = self.track_of(tile)
        {
            let card = crate::focus_card::Card {
                track,
                queued: look.queued,
                current: playing == Some(track.id),
                playing: look.playing,
                rect,
                scale,
                ink: folia_visualizer::color(theme.primary, 1.0),
                window_width: look.window_width,
            };
            let reveal = self.wall.focus.actions_reveal(Instant::now());
            layers = layers.push(crate::focus_card::body(i18n, card, reveal).map(Message::Card));
        }
        let local_error = self.tab == HomeTab::Local && self.layer.is_none() && self.error.is_some();
        if let (false, Some((title, body)), true) =
            (local_error, self.hint_text(i18n), self.wall.viewport().width > 1.0)
        {
            layers =
                layers.push(hint(title, body, theme, self.wall.seam(), self.wall.viewport(), hint_fade(self.hint_at)));
        }
        layers = layers.push(veil(daylight));
        if let (true, Some(error)) = (local_error, &self.error) {
            layers = layers.push(container(text(error).color(muted)).center(Length::Fill));
        }
        layers.into()
    }
}

/// The platform's short name, as the original's `providerLabel`.
pub fn provider_label(omni: Option<&Omni>) -> String {
    omni.and_then(|omni| omni.platforms().into_iter().find(|platform| platform.id == online::PROVIDER))
        .map_or_else(|| online::PROVIDER.to_owned(), |platform| platform.short_name.to_owned())
}

/// `libraryBravais.trackCount_one` / `_other`, as i18next picks a plural.
pub fn track_count(i18n: &I18n, count: u32) -> String {
    let key = if count == 1 { "libraryBravais.trackCount_one" } else { "libraryBravais.trackCount_other" };
    i18n.format(key, &[("count", &count.to_string())])
}

fn kind_label(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Playlist => "libraryBravais.kind.playlist",
        CollectionKind::Album => "libraryBravais.kind.album",
        CollectionKind::DailySongs | CollectionKind::PersonalFm => "libraryBravais.kind.feed",
    }
}

fn shelf_heading<'a>(shelf: &'a Shelf, i18n: &'a I18n) -> &'a str {
    match shelf {
        Shelf::DailySongs => i18n.t("home.dailyRecommendations"),
        Shelf::DailyPlaylists => i18n.t("home.neteaseDailyPlaylists"),
        Shelf::Charts => i18n.t("home.neteaseCharts"),
        Shelf::Block(title) => title,
    }
}

/// `summaryOfSection`: the count, then the kinds present, albums before playlists before songs.
fn section_summary(section: &folia_omni::ExploreSection, i18n: &I18n) -> String {
    let has = |wanted: &dyn Fn(&ExploreEntry) -> bool| section.entries.iter().any(wanted);
    let mut parts = vec![section.entries.len().to_string()];
    let album = |entry: &ExploreEntry| matches!(entry, ExploreEntry::Collection(c) if c.kind == CollectionKind::Album);
    let playlist =
        |entry: &ExploreEntry| matches!(entry, ExploreEntry::Collection(c) if c.kind != CollectionKind::Album);
    let song = |entry: &ExploreEntry| matches!(entry, ExploreEntry::Song(_));
    if has(&album) {
        parts.push(i18n.t("libraryBravais.kind.album").to_owned());
    }
    if has(&playlist) {
        parts.push(i18n.t("libraryBravais.kind.playlist").to_owned());
    }
    if has(&song) {
        parts.push(i18n.t("libraryBravais.kind.track").to_owned());
    }
    parts.join(" · ")
}

/// `buildOnlineRadioCards`: personal FM and the daily songs before the recommended playlists.
fn radio_cards(feed: &folia_omni::RadioFeed, i18n: &I18n) -> Vec<Collection> {
    let mut cards = Vec::new();
    let provider = online::PROVIDER.to_owned();
    if !feed.personal_fm.is_empty() {
        cards.push(Collection {
            provider: provider.clone(),
            id: "personal_fm".to_owned(),
            kind: CollectionKind::PersonalFm,
            name: i18n.t("home.personalFm").to_owned(),
            cover_url: feed.personal_fm.first().and_then(|track| track.cover_url.clone()),
            track_count: None,
            creator: None,
            // `personalFmModeLabel`: NetEase has FM modes, and the mode in use is the default one.
            description: Some(i18n.t("personalFmMode.mode.DEFAULT").to_owned()),
        });
    }
    cards.push(Collection {
        provider,
        id: "daily_recommendations".to_owned(),
        kind: CollectionKind::DailySongs,
        name: i18n.t("home.dailyRecommendations").to_owned(),
        cover_url: feed.daily_songs.first().and_then(|track| track.cover_url.clone()),
        track_count: Some(feed.daily_songs.len() as u32),
        creator: None,
        description: Some(i18n.t("home.dailyRecommendationsDescription").to_owned()),
    });
    cards.extend(feed.recommended.iter().cloned());
    cards
}

fn radio_description(collection: &Collection, i18n: &I18n) -> String {
    collection
        .description
        .clone()
        .or_else(|| collection.creator.clone())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| i18n.t("home.radio").to_owned())
}

/// `projectHomeCardItem`: a collection's count goes into its kind tag; personal FM is a poster with no count.
fn card_tile(collection: &Collection, description: String, i18n: &I18n) -> Tile {
    let kind = i18n.t(kind_label(collection.kind));
    let count = collection.track_count.filter(|count| *count > 0 && collection.kind != CollectionKind::PersonalFm);
    Tile {
        id: collection.wall_id(),
        title: collection.name.clone(),
        subtitle: description,
        badge: Some(count.map_or_else(|| kind.to_owned(), |count| format!("{kind} · {count}"))),
        cover: collection.cover_url.clone().map_or(CoverSource::None, CoverSource::Remote),
        heading: false,
        hue: wall::key_hue(&format!("card:{}:{}", card_type(collection.kind), collection.id)),
        stack: collection.kind != CollectionKind::PersonalFm,
    }
}

/// The card's `type`, as `homeCardItemKey` writes it into the item key.
fn card_type(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Playlist => "playlist",
        CollectionKind::Album => "album",
        CollectionKind::PersonalFm => "radio",
        CollectionKind::DailySongs => "daily_recommendations",
    }
}

fn track_tile(track: &Track, badge: Option<String>, i18n: &I18n) -> Tile {
    let cover = match (&track.cover_url, track.path.as_os_str().is_empty()) {
        (Some(url), _) => CoverSource::Remote(url.clone()),
        (None, false) => CoverSource::Local(track.path.clone()),
        (None, true) => CoverSource::None,
    };
    let artist = track.artist.clone().unwrap_or_else(|| {
        if track.online.is_some() { i18n.t("player.unknownArtist").to_owned() } else { String::new() }
    });
    let key = match &track.online {
        Some(online) => format!("online:{}:{}", online.provider, online.id),
        None => format!("local:{}", track.id),
    };
    Tile {
        id: track.id,
        title: track.title.clone(),
        subtitle: artist,
        badge,
        cover,
        heading: false,
        hue: wall::key_hue(&key),
        stack: false,
    }
}

/// `wall.css`: the theme background lit by an accent glow top-left and a secondary glow bottom-right.
fn ground<'a, M: 'a>(theme: &Theme) -> Element<'a, M> {
    let srgb = |rgb: folia_core::Rgb| [rgb.r, rgb.g, rgb.b].map(|channel| f32::from(channel) / 255.0);
    let glow = |rgb: folia_core::Rgb, alpha: f32, center: [f32; 2], reach: f32| {
        let [r, g, b] = srgb(rgb);
        Glow { center, reach, rgba: [r, g, b, alpha] }
    };
    let wash = Wash {
        base: srgb(theme.background),
        glows: [glow(theme.accent, 0.16, [0.14, 0.12], 0.29), glow(theme.secondary, 0.20, [0.86, 0.82], 0.32)],
    };
    shader(wash).width(Length::Fill).height(Length::Fill).into()
}

/// The wall's edge vignette (52% black at the corners, 16% by day) and its grain, over the tiles.
fn veil<'a, M: 'a>(daylight: bool) -> Element<'a, M> {
    let veil = if daylight { Veil { vignette: 0.16, grain: 0.019 } } else { Veil { vignette: 0.52, grain: 0.035 } };
    shader(veil).width(Length::Fill).height(Length::Fill).into()
}

/// Empty-wall copy (`BravaisWallHint`): the wider side of the seam, fading in over 0.32s.
fn hint<'a>(
    title: String,
    body: String,
    theme: &folia_core::Theme,
    seam: wall::Seam,
    view: iced::Size,
    fade: f32,
) -> Element<'a, Message> {
    let (origin, size, pad) = hint_frame(seam, view);
    let title_size = (view.width * 0.032).clamp(24.0, 48.0);
    let body_size = (view.width * 0.011).clamp(13.0, 16.0);
    let ink = folia_visualizer::color(theme.primary, 1.0);
    let rise = 6.0 * (1.0 - fade);
    let title = text(title)
        .size(title_size)
        .line_height(iced::widget::text::LineHeight::Relative(1.18))
        .font(crate::typeface::font(iced::font::Weight::Bold))
        .color(Color { a: 0.22 * fade, ..ink })
        .align_x(Alignment::Center)
        .width(Length::Fill);
    let body = text(body)
        .size(body_size)
        .line_height(iced::widget::text::LineHeight::Relative(1.65))
        .color(Color { a: 0.36 * fade, ..ink })
        .align_x(Alignment::Center)
        .width(Length::Fill);
    let block = column![title, body].spacing(title_size * 0.9).width(Length::Fixed(448.0)).align_x(Alignment::Center);
    let block = iced::widget::float(block).translate(move |_, _| iced::Vector::new(0.0, rise));
    let area = container(block)
        .width(Length::Fixed(size.0.max(1.0)))
        .height(Length::Fixed(size.1.max(1.0)))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(pad);
    container(area)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .padding(iced::Padding { top: origin.1, right: 0.0, bottom: 0.0, left: origin.0 })
        .into()
}

fn hint_frame(seam: wall::Seam, view: iced::Size) -> ((f32, f32), (f32, f32), iced::Padding) {
    let side = (view.width * 0.04).clamp(20.0, 64.0);
    if seam.horizontal {
        let (top, height) = wall::wider_span(seam.center, seam.width, view.height);
        ((0.0, top), (view.width, height), iced::Padding { top: 32.0, right: side, bottom: 96.0, left: side })
    } else {
        let (left, width) = wall::wider_span(seam.center, seam.width, view.width);
        ((left, 0.0), (width, view.height), iced::Padding { top: 72.0, right: side, bottom: 112.0, left: side })
    }
}

fn hint_fade(started: Option<Instant>) -> f32 {
    let Some(at) = started else { return 1.0 };
    let t = (Instant::now().saturating_duration_since(at).as_secs_f64() / 0.32).clamp(0.0, 1.0);
    // CSS `ease`: cubic-bezier(0.25, 0.1, 0.25, 1).
    folia_visualizer::Ease::new(0.25, 0.1, 0.25, 1.0).at(t) as f32
}

fn load(db_path: PathBuf, folder: Option<PathBuf>, scan: bool) -> Task<Message> {
    let work = blocking(move || {
        let mut library = Library::open(&db_path)?;
        if let Some(folder) = &folder {
            library.add_folder(folder)?;
        }
        if scan {
            library.scan()?;
        }
        library.tracks()
    });
    Task::perform(work, |result| {
        Message::Loaded(match result {
            Some(Ok(tracks)) => Ok(tracks),
            Some(Err(err)) => {
                tracing::error!(error = &err as &dyn std::error::Error, "library load failed");
                Err(error_chain(&err))
            }
            None => Err("library worker stopped".to_owned()),
        })
    })
}
