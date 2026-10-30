// Lock-free state shared by the control side, the decoder thread and the audio callback.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerState {
    Idle,
    Playing,
    Paused,
    Ended,
    Failed,
}

impl PlayerState {
    fn from_u8(value: u8) -> PlayerState {
        match value {
            1 => PlayerState::Playing,
            2 => PlayerState::Paused,
            3 => PlayerState::Ended,
            4 => PlayerState::Failed,
            _ => PlayerState::Idle,
        }
    }
}

/// An `f64` stored as bits in an `AtomicU64`.
#[derive(Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub fn load(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Acquire))
    }

    pub fn store(&self, value: f64) {
        self.0.store(value.to_bits(), Ordering::Release);
    }
}

pub struct Shared {
    state: AtomicU8,
    volume: AtomicU32,
    /// Seconds of the track at the first sample after the last flush.
    base: AtomicF64,
    /// Base the callback adopts when it acknowledges the pending flush.
    pending_base: AtomicF64,
    /// Frames the callback has played since the last flush.
    played_frames: AtomicU64,
    flush_requested: AtomicU64,
    flush_done: AtomicU64,
    /// Set by the decoder once every sample of the track is in the ring buffer.
    pub decoder_done: AtomicBool,
    pub duration: AtomicF64,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(PlayerState::Idle as u8),
            volume: AtomicU32::new(1f32.to_bits()),
            base: AtomicF64::default(),
            pending_base: AtomicF64::default(),
            played_frames: AtomicU64::new(0),
            flush_requested: AtomicU64::new(0),
            flush_done: AtomicU64::new(0),
            decoder_done: AtomicBool::new(false),
            duration: AtomicF64::default(),
        }
    }

    pub fn state(&self) -> PlayerState {
        PlayerState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, state: PlayerState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn position(&self, rate: u32) -> f64 {
        self.base.load() + self.played_frames.load(Ordering::Acquire) as f64 / f64::from(rate)
    }

    /// Asks the callback to drop buffered samples; playback then reports `base` as its position.
    pub fn request_flush(&self, base: f64) -> u64 {
        self.pending_base.store(base);
        self.flush_requested.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn flush_done(&self, epoch: u64) -> bool {
        self.flush_done.load(Ordering::Acquire) >= epoch
    }

    /// Called by the audio callback: returns true when a flush is pending and marks it done.
    pub fn take_flush(&self) -> bool {
        let requested = self.flush_requested.load(Ordering::Acquire);
        if self.flush_done.load(Ordering::Acquire) == requested {
            return false;
        }
        self.base.store(self.pending_base.load());
        self.played_frames.store(0, Ordering::Release);
        self.flush_done.store(requested, Ordering::Release);
        true
    }

    pub fn add_played(&self, frames: u64) {
        self.played_frames.fetch_add(frames, Ordering::AcqRel);
    }
}
