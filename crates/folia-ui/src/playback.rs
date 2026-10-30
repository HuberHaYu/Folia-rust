// Playback controller: owns the player and the queue, follows track ends and loads lyrics.

use folia_core::{Lyrics, Queue, Repeat, Track};
use folia_player::{Player, PlayerState, Status};
use folia_store::{PlaybackSettings, Store};
use iced::Task;

use crate::background::{blocking, save};

pub struct Playback {
    player: Option<Player>,
    pub queue: Queue<Track>,
    pub lyrics: Option<Lyrics>,
    pub status: Status,
    pub settings: PlaybackSettings,
    seeking: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle,
    Next,
    Previous,
    Seeking(f64),
    SeekDone,
    Volume(f32),
    VolumeDone,
    CycleRepeat,
    Tick,
    LyricsLoaded(i64, Option<Lyrics>),
}

impl Playback {
    pub fn new(settings: PlaybackSettings) -> Self {
        let player = match Player::new() {
            Ok(player) => {
                player.set_volume(settings.volume);
                Some(player)
            }
            Err(err) => {
                tracing::error!(error = &err as &dyn std::error::Error, "audio output unavailable");
                None
            }
        };
        let mut queue = Queue::default();
        queue.repeat = settings.repeat;
        let status = Status { state: PlayerState::Idle, position: 0.0, duration: 0.0 };
        Self { player, queue, lyrics: None, status, settings, seeking: None }
    }

    pub fn current(&self) -> Option<&Track> {
        self.queue.current()
    }

    pub fn is_playing(&self) -> bool {
        self.status.state == PlayerState::Playing
    }

    /// The position to show: the slider's value while it is being dragged.
    pub fn position(&self) -> f64 {
        self.seeking.unwrap_or(self.status.position)
    }

    pub fn duration(&self) -> f64 {
        if self.status.duration > 0.0 {
            self.status.duration
        } else {
            self.current().map_or(0.0, |track| track.duration)
        }
    }

    /// Replaces the queue with `tracks` and plays the one at `index`.
    pub fn play(&mut self, tracks: Vec<Track>, index: usize) -> Task<Message> {
        self.queue.replace(tracks, index);
        self.start_current()
    }

    pub fn update(&mut self, message: Message, store: &Store) -> Task<Message> {
        match message {
            Message::Toggle => {
                match self.status.state {
                    PlayerState::Playing => self.with_player(Player::pause),
                    PlayerState::Paused => self.with_player(Player::resume),
                    _ if self.current().is_some() => return self.start_current(),
                    _ => {}
                }
                self.refresh();
                Task::none()
            }
            Message::Next => self.skip(true),
            Message::Previous => self.skip(false),
            Message::Seeking(position) => {
                self.seeking = Some(position);
                Task::none()
            }
            Message::SeekDone => {
                if let (Some(position), Some(player)) = (self.seeking.take(), &self.player) {
                    player.seek(position);
                }
                Task::none()
            }
            Message::Volume(volume) => {
                self.settings.volume = volume;
                self.with_player(|player| player.set_volume(volume));
                Task::none()
            }
            Message::VolumeDone => save(store, &self.settings),
            Message::CycleRepeat => {
                self.settings.repeat = match self.settings.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.queue.repeat = self.settings.repeat;
                save(store, &self.settings)
            }
            Message::Tick => self.tick(),
            Message::LyricsLoaded(id, lyrics) => {
                if self.current().is_some_and(|track| track.id == id) {
                    self.lyrics = lyrics;
                }
                Task::none()
            }
        }
    }

    fn tick(&mut self) -> Task<Message> {
        self.refresh();
        match self.status.state {
            PlayerState::Ended => self.advance(true),
            PlayerState::Failed => self.advance(false),
            _ => Task::none(),
        }
    }

    fn skip(&mut self, forward: bool) -> Task<Message> {
        if forward {
            return self.advance(false);
        }
        // Like most players, "previous" restarts the track unless it has only just begun.
        if self.status.position > 3.0 {
            self.with_player(|player| player.seek(0.0));
            return Task::none();
        }
        if self.queue.back().is_some() { self.start_current() } else { Task::none() }
    }

    fn advance(&mut self, finished: bool) -> Task<Message> {
        if self.queue.advance(finished).is_some() {
            return self.start_current();
        }
        self.with_player(Player::stop);
        self.refresh();
        Task::none()
    }

    fn start_current(&mut self) -> Task<Message> {
        let Some(track) = self.queue.current().cloned() else { return Task::none() };
        self.lyrics = None;
        self.seeking = None;
        self.with_player(|player| player.load(&track.path));
        self.refresh();
        tracing::info!(track = %track.display_name(), "playing");
        Task::perform(blocking(move || folia_library::lyrics_for(&track.path)), move |lyrics| {
            Message::LyricsLoaded(track.id, lyrics.flatten())
        })
    }

    fn refresh(&mut self) {
        if let Some(player) = &self.player {
            self.status = player.status();
        }
    }

    fn with_player(&self, action: impl FnOnce(&Player)) {
        if let Some(player) = &self.player {
            action(player);
        }
    }
}
