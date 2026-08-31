//! Getting the beeper out of the machine and into a speaker.
//!
//! The emulation is in the library and tested there; this is only the seam. And the seam is
//! the awkward part: the machine makes 19.968 ms of sound in one burst at the end of a
//! frame, while the audio device asks for whatever it wants, whenever it wants it. A queue
//! between them absorbs the difference, and is trimmed when it grows — latency is worse
//! than a dropped sample.
//!
//! If there is no device, or it will not take f32, the emulator runs on in silence. Sound
//! is never a reason to fail to start.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use on_the_spectrum::spectrum::beeper::Beeper;

use crate::Emulator;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        match Audio::start() {
            Ok(audio) => {
                let rate = audio.sample_rate;
                info!("audio: {rate} Hz, {} channels", audio.channels);
                app.insert_non_send(audio)
                    // The beeper generates at whatever rate the device asked for, so
                    // nothing has to resample afterwards.
                    .add_systems(Startup, move |mut emu: ResMut<Emulator>| {
                        emu.machine.bus.beeper = Beeper::new(rate);
                    })
                    .add_systems(Update, feed);
            }
            // Ordering against `run_emulation` does not matter: draining a frame late
            // costs one frame of latency and nothing else.
            Err(e) => warn!("no sound: {e}"),
        }
    }
}

pub struct Audio {
    /// Dropping this stops the sound, so it has to be held onto.
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl Audio {
    fn start() -> Result<Audio, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no output device")?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("no usable config: {e}"))?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "the device wants {:?}, not f32",
                config.sample_format()
            ));
        }

        let sample_rate = config.sample_rate();
        let channels = config.channels() as usize;
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));

        let playing = Arc::clone(&queue);
        let stream = device
            .build_output_stream(
                config.config(),
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // A poisoned lock would mean the emulator panicked mid-push; the
                    // samples are still samples, so carry on rather than kill the audio.
                    let mut queue = playing.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in out.chunks_mut(channels) {
                        // Nothing queued is silence, not a repeat: a stutter is easier to
                        // listen past than a buzz.
                        let sample = queue.pop_front().unwrap_or(0.0);
                        frame.fill(sample);
                    }
                },
                |e| error!("audio stream: {e}"),
                None,
            )
            .map_err(|e| format!("could not open the device: {e}"))?;
        stream.play().map_err(|e| format!("could not start: {e}"))?;

        Ok(Audio {
            _stream: stream,
            queue,
            sample_rate,
            channels,
        })
    }

    fn push(&self, samples: &[f32]) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.extend(samples.iter().copied());
        // Getting ahead of the device is latency, and a hundred milliseconds of it is
        // already enough to hear against the picture. Drop the oldest.
        let limit = self.sample_rate as usize / 10;
        if queue.len() > limit {
            let excess = queue.len() - limit;
            queue.drain(..excess);
        }
    }
}

fn feed(audio: NonSend<Audio>, mut emu: ResMut<Emulator>) {
    let samples = emu.machine.bus.beeper.take();
    if !samples.is_empty() {
        audio.push(&samples);
    }
}
