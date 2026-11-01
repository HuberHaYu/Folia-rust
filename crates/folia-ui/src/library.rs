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
    Fm,
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
}

/// What the shell does after a page message.
pub enum Effect {
    Play { queue: Vec<Track>, index: usize, origin: Option<String> },
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
}

#[derive(Debug, Clone)]
pub enum Message {
    Import,
    Picked(Option<PathBuf>),
    Reload,
    Loaded(Result<Vec<Track>, String>),
    Wall(wall::Message),
    Frame,
    Tab(HomeTab),
    Online(online::Message),
    Opened(String, Result<Vec<Track>, String>),
    Back,
    PlayAll,
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
        self.wall.coasting()
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
    pub fn reveal(&mut self, index: usize) {
        if self.tab == HomeTab::Local && self.layer.is_none() {
            self.wall.center_on(index);
        }
    }

    /// Widens the seam into the full info strip while a collection layer is open.
    pub fn set_tab_chrome(&self, home: &mut crate::home::HomeChrome, now: Instant) {
        home.set_collection(self.layer.is_some(), now);
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
                    Some(index) => {
                        let (more, effect) = self.press(index, i18n);
                        (Task::batch([task, more]), effect)
                    }
                    None => (task, None),
                }
            }
            Message::Frame => {
                self.wall.tick_coast(Instant::now());
                (Task::none(), None)
            }
            Message::Tab(tab) => {
                if tab == self.tab && self.layer.is_none() {
                    return (Task::none(), None);
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
                    Ok(tracks) => Feed::Ready(tracks),
                    Err(err) => {
                        tracing::warn!(error = %err, "collection unavailable");
                        Feed::Failed(err)
                    }
                };
                (self.rebuild(i18n), None)
            }
            Message::Back => {
                self.layer = None;
                (self.rebuild(i18n), None)
            }
            Message::PlayAll => {
                let Some(layer) = &self.layer else { return (Task::none(), None) };
                let Some(tracks) = layer.tracks.ready().filter(|tracks| !tracks.is_empty()) else {
                    return (Task::none(), None);
                };
                let effect =
                    Effect::Play { queue: tracks.clone(), index: 0, origin: Some(layer.collection.name.clone()) };
                (Task::none(), Some(effect))
            }
        }
    }

    /// What pressing the tile at `index` does: play from it, or open its collection as a layer.
    fn press(&mut self, index: usize, i18n: &I18n) -> (Task<Message>, Option<Effect>) {
        match self.actions.get(index).cloned().unwrap_or(Action::None) {
            Action::Local(index) => {
                (Task::none(), Some(Effect::Play { queue: self.tracks.clone(), index, origin: None }))
            }
            Action::LayerTrack(index) => {
                let Some(layer) = &self.layer else { return (Task::none(), None) };
                let Some(tracks) = layer.tracks.ready() else { return (Task::none(), None) };
                let origin = Some(layer.collection.name.clone());
                (Task::none(), Some(Effect::Play { queue: tracks.clone(), index, origin }))
            }
            Action::Fm => {
                let Some(feed) = self.online.radio.ready().filter(|feed| !feed.personal_fm.is_empty()) else {
                    return (Task::none(), None);
                };
                let origin = Some(i18n.t("home.personalFm").to_owned());
                (Task::none(), Some(Effect::Play { queue: feed.personal_fm.clone(), index: 0, origin }))
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
                (Task::none(), Some(Effect::Play { queue: songs, index: song, origin }))
            }
            Action::Open(collection) => (self.open(collection, i18n), None),
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
                let work = async move { omni.collection_tracks(&wanted).await.map_err(|err| error_chain(&err)) };
                Task::perform(work, move |result| Message::Opened(id.clone(), result))
            }
            (None, None) => Task::none(),
        };
        let tracks = daily.map_or(Feed::Loading, Feed::Ready);
        self.layer = Some(Layer { collection, tracks });
        Task::batch([fetch, self.rebuild(i18n)])
    }

    /// Builds the tiles of the current layer or tab and lays them on the wall.
    fn rebuild(&mut self, i18n: &I18n) -> Task<Message> {
        let (tiles, actions, shape) = self.compose(i18n);
        self.tiles = tiles;
        self.actions = actions;
        self.wall.relayout(&self.tiles, shape, self.online.omni()).map(Message::Wall)
    }

    fn compose(&self, i18n: &I18n) -> (Vec<Tile>, Vec<Action>, Shape) {
        if let Some(layer) = &self.layer {
            let tracks = layer.tracks.ready().map(Vec::as_slice).unwrap_or_default();
            let tiles = tracks.iter().map(|track| track_tile(track, None, i18n)).collect();
            return (tiles, (0..tracks.len()).map(Action::LayerTrack).collect(), Shape::Ranked);
        }
        if self.tab == HomeTab::Local {
            let tiles = self.tracks.iter().map(|track| track_tile(track, None, i18n)).collect();
            return (tiles, (0..self.tracks.len()).map(Action::Local).collect(), Shape::Ranked);
        }
        if self.online.signed_in().is_none() {
            return (Vec::new(), Vec::new(), Shape::Ranked);
        }
        let mut tiles = Vec::new();
        let mut actions = Vec::new();
        let card = |collection: &Collection, description: String, tiles: &mut Vec<Tile>, actions: &mut Vec<Action>| {
            tiles.push(card_tile(collection, description, i18n));
            actions.push(if collection.kind == CollectionKind::PersonalFm {
                Action::Fm
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
        (tiles, actions, Shape::Ranked)
    }

    /// `projectExploreItems` and `projectExploreHeadings`: entries first, then one heading tile per section.
    fn compose_explore(&self, i18n: &I18n) -> (Vec<Tile>, Vec<Action>, Shape) {
        let Some(sections) = self.online.explore.ready() else { return (Vec::new(), Vec::new(), Shape::Ranked) };
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
            });
            actions.push(Action::None);
            shape.push(lots::Section { key, heading, entries });
        }
        (tiles, actions, Shape::Explore(shape))
    }

    /// The seam's status line (`BravaisHomeDirectory` status).
    pub fn status(&self, i18n: &I18n) -> Option<String> {
        if let Some(layer) = &self.layer {
            return match &layer.tracks {
                Feed::Loading | Feed::Idle => Some(i18n.t("playlist.loading").to_owned()),
                Feed::Failed(_) => Some(i18n.t("home.loadingLibrary").to_owned()),
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

    /// The open collection's title, count line and kind, for the seam.
    pub fn layer_meta(&self, i18n: &I18n) -> Option<(String, String)> {
        let layer = self.layer.as_ref()?;
        let count = layer.tracks.ready().map(|tracks| tracks.len() as u32).or(layer.collection.track_count);
        let kind = i18n.t(kind_label(layer.collection.kind)).to_owned();
        let meta = match count {
            Some(count) => format!("{kind} · {}", track_count(i18n, count)),
            None => kind,
        };
        Some((layer.collection.name.clone(), meta))
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
        _text_color: Color,
        muted: Color,
        playing: Option<i64>,
    ) -> Element<'a, Message> {
        let wall: Element<'a, wall::Message> = canvas(self.wall.view(&self.tiles, theme, daylight, playing))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let mut layers = stack![ground(theme), wall.map(Message::Wall)];
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
            description: Some(i18n.t("home.personalFm").to_owned()),
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
    Tile { id: track.id, title: track.title.clone(), subtitle: artist, badge, cover, heading: false }
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
        .font(iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT })
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
