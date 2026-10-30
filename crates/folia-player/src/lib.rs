// Audio playback: a decoder thread feeds a lock-free ring buffer that the device callback drains.

mod convert;
mod decoder;
mod output;
mod shared;
mod source;

pub use convert::map_channels;
pub use shared::PlayerState;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use cpal::traits::StreamTrait;

use crate::decoder::{Command, Decoder};
use crate::shared::Shared;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("no audio output device")]
    NoDevice,
    #[error("audio output device error")]
    Device(#[source] cpal::Error),
    #[error("unsupported output sample format {0}")]
    UnsupportedFormat(String),
    #[error("cannot open {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("file has no playable audio track")]
    NoAudioTrack,
    #[error("cannot decode audio")]
    Decode(#[from] symphonia::core::errors::Error),
    #[error("cannot resample audio: {0}")]
    Resample(String),
    #[error("cannot start the decoder thread")]
    Thread(#[source] std::io::Error),
}

/// What the UI needs to draw the transport: state, position and length in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    pub state: PlayerState,
    pub position: f64,
    pub duration: f64,
}

pub struct Player {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    stream: cpal::Stream,
    rate: u32,
}

impl Player {
    pub fn new() -> Result<Self, PlayerError> {
        let shared = Arc::new(Shared::new());
        let (output, producer) = output::open(shared.clone())?;
        let (commands, receiver) = mpsc::channel();
        let decoder = Decoder::new(shared.clone(), producer, output.channels, output.rate, receiver);
        std::thread::Builder::new()
            .name("folia-decoder".into())
            .spawn(move || decoder.run())
            .map_err(PlayerError::Thread)?;
        tracing::info!(rate = output.rate, channels = output.channels, "audio output opened");
        Ok(Self { commands, shared, stream: output.stream, rate: output.rate })
    }

    /// Starts playing `path` from the beginning.
    pub fn load(&self, path: &Path) {
        // Mark the new track as playing now, so a status read before the decoder starts never sees the old end.
        self.shared.decoder_done.store(false, std::sync::atomic::Ordering::Release);
        self.shared.set_state(PlayerState::Playing);
        self.run_stream(true);
        self.send(Command::Load(path.to_owned()));
    }

    pub fn pause(&self) {
        if self.shared.state() == PlayerState::Playing {
            self.shared.set_state(PlayerState::Paused);
        }
    }

    pub fn resume(&self) {
        if self.shared.state() == PlayerState::Paused {
            self.shared.set_state(PlayerState::Playing);
        }
    }

    pub fn seek(&self, seconds: f64) {
        self.run_stream(true);
        self.send(Command::Seek(seconds));
    }

    pub fn stop(&self) {
        self.shared.set_state(PlayerState::Idle);
        self.send(Command::Stop);
        self.run_stream(false);
    }

    pub fn volume(&self) -> f32 {
        self.shared.volume()
    }

    /// Sets the output gain, clamped to `0.0..=1.0`.
    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    pub fn status(&self) -> Status {
        let state = self.shared.state();
        let duration = self.shared.duration.load();
        let position = match state {
            PlayerState::Idle => 0.0,
            _ => self.shared.position(self.rate).min(if duration > 0.0 { duration } else { f64::MAX }),
        };
        Status { state, position, duration }
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("decoder thread has stopped");
        }
    }

    // The stream runs while a track is loaded, paused included, so flushes are always acknowledged.
    fn run_stream(&self, running: bool) {
        let result = if running { self.stream.play() } else { self.stream.pause() };
        if let Err(err) = result {
            tracing::warn!(error = &err as &dyn std::error::Error, running, "cannot change output stream state");
        }
    }
}
