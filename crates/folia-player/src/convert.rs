// Converts decoded blocks to the device's channel count and sample rate.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::PlayerError;
use crate::source::Block;

const CHUNK_FRAMES: usize = 1024;

pub struct Converter {
    channels: usize,
    rate: u32,
    resampler: Option<(u32, Fft<f32>)>,
    mapped: Vec<f32>,
    resampled: Vec<f32>,
}

impl Converter {
    pub fn new(channels: usize, rate: u32) -> Self {
        Self { channels, rate, resampler: None, mapped: Vec::new(), resampled: Vec::new() }
    }

    /// Drops buffered input, for a new track or after a seek.
    pub fn reset(&mut self) {
        self.mapped.clear();
        if let Some((_, resampler)) = &mut self.resampler {
            resampler.reset();
        }
    }

    /// Appends the block, converted to the device format, to `out`.
    pub fn push(&mut self, block: &Block<'_>, out: &mut Vec<f32>) -> Result<(), PlayerError> {
        map_channels(block.samples, block.channels, self.channels, &mut self.mapped);
        if block.rate == self.rate {
            out.append(&mut self.mapped);
            return Ok(());
        }

        let resampler = match &mut self.resampler {
            Some((rate, resampler)) if *rate == block.rate => resampler,
            slot => {
                let resampler =
                    Fft::new(block.rate as usize, self.rate as usize, CHUNK_FRAMES, self.channels, FixedSync::Input)
                        .map_err(|err| PlayerError::Resample(err.to_string()))?;
                &mut slot.insert((block.rate, resampler)).1
            }
        };

        let channels = self.channels;
        let mut consumed = 0;
        while (self.mapped.len() - consumed) / channels >= resampler.input_frames_next() {
            let frames_in = resampler.input_frames_next();
            let frames_out = resampler.output_frames_max();
            self.resampled.resize(frames_out * channels, 0.0);
            let input = &self.mapped[consumed..consumed + frames_in * channels];
            let input = InterleavedSlice::new(input, channels, frames_in).map_err(resample_error)?;
            let mut output =
                InterleavedSlice::new_mut(&mut self.resampled, channels, frames_out).map_err(resample_error)?;
            let (used, written) = resampler.process_into_buffer(&input, &mut output, None).map_err(resample_error)?;
            out.extend_from_slice(&self.resampled[..written * channels]);
            consumed += used * channels;
        }
        self.mapped.drain(..consumed);
        Ok(())
    }
}

fn resample_error(err: impl std::fmt::Display) -> PlayerError {
    PlayerError::Resample(err.to_string())
}

/// Appends `src` with `from` channels to `out` as `to` channels: mono is copied to every channel,
/// extra source channels are dropped and missing ones repeat the last source channel.
pub fn map_channels(src: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    if from == to {
        out.extend_from_slice(src);
        return;
    }
    out.reserve(src.len() / from.max(1) * to);
    for frame in src.chunks_exact(from.max(1)) {
        out.extend((0..to).map(|channel| frame[channel.min(frame.len() - 1)]));
    }
}
