// Playback controller: owns the player and the queue, follows track ends and loads lyrics.

use std::path::PathBuf;

use folia_core::{Lyrics, Queue, Repeat, Track};
use folia_omni::Omni;
use folia_player::{Player, PlayerState, Status};
use folia_store::{PlaybackSettings, Store};
use iced::Task;

use crate::background::{blocking, error_chain, save};

pub struct Playback {
    player: Option<Player>,
    omni: Option<Omni>,
    pub queue: Queue<Track>,
    pub lyrics: Option<Lyrics>,
    pub status: Status,
    pub settings: PlaybackSettings,
    seeking: Option<f64>,
    /// Tracks that failed one after another; playback stops once the whole queue has failed.
    failures: usize,
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
    AudioReady(i64, Result<PathBuf, String>),
}

impl Playback {
    pub fn new(settings: PlaybackSettings, omni: Option<Omni>) -> Self {
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
        Self { player, omni, queue, lyrics: None, status, settings, seeking: None, failures: 0 }
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
            Message::AudioReady(id, result) => {
                let Some(track) = self.queue.current_mut().filter(|track| track.id == id) else { return Task::none() };
                match result {
                    Ok(path) => {
                        self.failures = 0;
                        track.path = path;
                        let path = track.path.clone();
                        self.with_player(|player| player.load(&path));
                        self.refresh();
                        Task::none()
                    }
                    Err(err) => {
                        tracing::warn!(track = %track.display_name(), error = %err, "online track unavailable");
                        self.fail()
                    }
                }
            }
        }
    }

    fn tick(&mut self) -> Task<Message> {
        self.refresh();
        match self.status.state {
            PlayerState::Ended => self.advance(true),
            PlayerState::Failed => self.fail(),
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

    fn fail(&mut self) -> Task<Message> {
        self.failures += 1;
        if self.failures >= self.queue.items().len() {
            self.failures = 0;
            self.with_player(Player::stop);
            self.refresh();
            return Task::none();
        }
        self.advance(false)
    }

    fn advance(&mut self, finished: bool) -> Task<Message> {
        if self.queue.advance(finished).is_some() {
            return self.start_current();
        }
        self.with_player(Player::stop);
        self.refresh();
        Task::none()
    }

    /// Starts the current track: local files play at once, online tracks are downloaded first.
    fn start_current(&mut self) -> Task<Message> {
        let Some(track) = self.queue.current().cloned() else { return Task::none() };
        self.lyrics = None;
        self.seeking = None;
        tracing::info!(track = %track.display_name(), "playing");
        let id = track.id;
        let (Some(online), Some(omni)) = (track.online.clone(), self.omni.clone()) else {
            self.with_player(|player| player.load(&track.path));
            self.refresh();
            return Task::perform(blocking(move || folia_library::lyrics_for(&track.path)), move |lyrics| {
                Message::LyricsLoaded(id, lyrics.flatten())
            });
        };

        self.with_player(Player::stop);
        self.refresh();
        let (lyrics_omni, lyrics_id) = (omni.clone(), online.clone());
        let lyrics = Task::perform(async move { lyrics_omni.lyrics(&lyrics_id).await }, move |result| {
            Message::LyricsLoaded(
                id,
                result.unwrap_or_else(|err| {
                    tracing::warn!(error = &err as &dyn std::error::Error, "online lyrics unavailable");
                    None
                }),
            )
        });
        let audio = Task::perform(async move { omni.fetch_audio(&online).await }, move |result| {
            Message::AudioReady(id, result.map_err(|err| error_chain(&err)))
        });
        Task::batch([audio, lyrics])
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
