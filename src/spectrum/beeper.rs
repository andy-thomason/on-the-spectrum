//! The beeper: one bit of port `0xFE`, turned into samples.
//!
//! There is no sound chip in a 48K. Bit 4 of `OUT (0xFE),A` drives a small speaker
//! directly, so every note a program plays is the CPU flipping that bit at the right
//! instant — which is why sound and game logic compete for the same T-states, and why the
//! only thing this needs to record is *when* the bit changed.
//!
//! Samples are **integrated, not point-sampled**: each one is the average level across the
//! T-states it covers. Point-sampling a 1-bit source at 44.1 kHz turns a 540 Hz square
//! wave into an aliased screech, because the transitions land between samples.
//!
//! Measured against Manic Miner, the numbers this has to cope with are in
//! [`doc/get-a-game-running.md`](../../../doc/get-a-game-running.md) §5: half-cycles down
//! to 3240 T-states, and about 98% of writes to the port not moving the speaker at all.

/// CD-quality is far more than a 1-bit speaker needs, but it is what audio devices want.
pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;
/// The Z80's clock, and so the units transitions arrive in.
pub const CPU_HZ: f64 = 3_500_000.0;

pub struct Beeper {
    sample_rate: u32,
    /// T-states per sample: 79.365 at 44.1 kHz, and the fraction matters — rounding it
    /// would drift by a whole sample every couple of hundred.
    t_per_sample: f64,
    /// Where the integrator has reached, in absolute T-states.
    cursor: f64,
    /// The T-state at which the sample being accumulated ends.
    window_end: f64,
    /// Speaker level now.
    level: bool,
    /// T-states the speaker has been high for within the current sample.
    high: f64,
    /// How loud, 0.0 to 1.0. The real machine had no volume control; this is for us.
    pub volume: f32,
    /// The DC blocker's state. A speaker at rest is silent, but a *level* is not zero, so
    /// without this every sample of silence carries a constant offset and every sound
    /// starts and ends with a thump. A real speaker cannot hold a displacement either.
    dc_in: f32,
    dc_out: f32,
    /// The filter has seen a sample, so it knows what "no change" looks like.
    started: bool,
    samples: Vec<f32>,
    /// Samples to keep if nobody is draining them. About a second at 44.1 kHz.
    capacity: usize,
}

impl Default for Beeper {
    fn default() -> Self {
        Beeper::new(DEFAULT_SAMPLE_RATE)
    }
}

impl Beeper {
    pub fn new(sample_rate: u32) -> Self {
        let t_per_sample = CPU_HZ / sample_rate as f64;
        Beeper {
            sample_rate,
            t_per_sample,
            cursor: 0.0,
            window_end: t_per_sample,
            level: false,
            high: 0.0,
            volume: 0.25,
            dc_in: 0.0,
            dc_out: 0.0,
            started: false,
            samples: Vec::new(),
            capacity: sample_rate as usize,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The speaker changed level at absolute T-state `t`.
    pub fn write(&mut self, t: u64, level: bool) {
        self.advance_to(t);
        self.level = level;
    }

    /// Bring the integrator up to `t`, emitting whatever samples are now complete.
    ///
    /// Called once per frame as well as on every transition, so audio is produced steadily
    /// whether a program is making a sound or not.
    pub fn advance_to(&mut self, t: u64) {
        let t = t as f64;
        if t < self.cursor {
            return; // the machine was reset or a snapshot rewound the clock
        }
        while self.window_end <= t {
            if self.level {
                self.high += self.window_end - self.cursor;
            }
            let duty = self.high / self.t_per_sample;
            let sample = (duty as f32 * 2.0 - 1.0) * self.volume;
            let blocked = self.block_dc(sample);
            self.samples.push(blocked);
            self.cursor = self.window_end;
            self.window_end += self.t_per_sample;
            self.high = 0.0;
        }
        if self.level {
            self.high += t - self.cursor;
        }
        self.cursor = t;

        // Nobody is listening; keep the most recent second and drop the rest.
        if self.samples.len() > self.capacity {
            let excess = self.samples.len() - self.capacity;
            self.samples.drain(..excess);
        }
    }

    /// A one-pole high-pass at about 7 Hz: it passes everything audible and lets a held
    /// level decay to nothing.
    fn block_dc(&mut self, sample: f32) -> f32 {
        const R: f32 = 0.999;
        if !self.started {
            // Start from wherever the speaker already is, so switching on is not a thump.
            self.started = true;
            self.dc_in = sample;
        }
        self.dc_out = sample - self.dc_in + R * self.dc_out;
        self.dc_in = sample;
        self.dc_out
    }

    /// Take the samples generated so far.
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    pub fn pending(&self) -> usize {
        self.samples.len()
    }

    /// Keep this many samples when nobody is draining them. A recording wants all of
    /// them; a live machine wants a second at most.
    pub fn set_capacity(&mut self, samples: usize) {
        self.capacity = samples;
    }

    /// Start again from silence, keeping the sample rate — for a reset or a snapshot load.
    pub fn reset(&mut self) {
        let volume = self.volume;
        *self = Beeper::new(self.sample_rate);
        self.volume = volume;
    }
}

/// Samples as a 16-bit mono WAV, so a headless run can be listened to.
pub fn wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // the size of this chunk
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &sample in samples {
        let clamped = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&clamped.to_le_bytes());
    }
    out
}
