// The cpal output stream: the real-time callback copies decoded samples from the ring buffer to the device.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::PlayerError;
use crate::shared::{PlayerState, Shared};

pub struct Output {
    pub stream: Stream,
    pub rate: u32,
    pub channels: usize,
}

/// Seconds of audio the ring buffer between decoder and device holds.
const BUFFER_SECONDS: f64 = 0.5;

/// Opens the default output device and returns the stream with the producer end of its ring buffer.
pub fn open(shared: Arc<Shared>) -> Result<(Output, Producer<f32>), PlayerError> {
    let device = cpal::default_host().default_output_device().ok_or(PlayerError::NoDevice)?;
    let supported = device.default_output_config().map_err(PlayerError::Device)?;
    let config = supported.config();
    let channels = usize::from(config.channels);
    let capacity = (f64::from(config.sample_rate) * BUFFER_SECONDS) as usize * channels;
    let (producer, consumer) = RingBuffer::new(capacity);

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, consumer, shared),
        SampleFormat::F64 => build::<f64>(&device, config, consumer, shared),
        SampleFormat::I16 => build::<i16>(&device, config, consumer, shared),
        SampleFormat::I32 => build::<i32>(&device, config, consumer, shared),
        SampleFormat::U16 => build::<u16>(&device, config, consumer, shared),
        other => return Err(PlayerError::UnsupportedFormat(format!("{other:?}"))),
    }?;
    Ok((Output { stream, rate: config.sample_rate, channels }, producer))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut consumer: Consumer<f32>,
    shared: Arc<Shared>,
) -> Result<Stream, PlayerError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let callback = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        if shared.take_flush() {
            let stale = consumer.slots();
            if let Ok(chunk) = consumer.read_chunk(stale) {
                chunk.commit_all();
            }
        }
        if shared.state() != PlayerState::Playing {
            out.fill(T::EQUILIBRIUM);
            return;
        }

        let volume = shared.volume();
        let available = consumer.slots().min(out.len());
        let available = available - available % channels;
        if let Ok(chunk) = consumer.read_chunk(available) {
            let (first, second) = chunk.as_slices();
            for (dst, &src) in out.iter_mut().zip(first.iter().chain(second)) {
                *dst = T::from_sample(src * volume);
            }
            chunk.commit_all();
        }
        out[available..].fill(T::EQUILIBRIUM);
        shared.add_played((available / channels) as u64);

        if available == 0 && shared.decoder_done.load(std::sync::atomic::Ordering::Acquire) {
            shared.set_state(PlayerState::Ended);
        }
    };
    let on_error = |err: cpal::Error| tracing::error!(error = &err as &dyn std::error::Error, "audio output error");
    device.build_output_stream::<T, _, _>(config, callback, on_error, None).map_err(PlayerError::Device)
}
