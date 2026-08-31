//! The beeper: one bit in, samples out.
//!
//! The properties worth pinning down are the ones that are easy to get subtly wrong — the
//! fractional sample rate not drifting, a held level not sitting as a DC offset, and
//! transitions faster than the sample rate coming out quiet rather than as an aliased
//! screech.

use on_the_spectrum::spectrum::beeper::{Beeper, CPU_HZ, DEFAULT_SAMPLE_RATE, wav};

/// Drive a square wave of `half_cycle` T-states for `seconds`, and return the samples.
fn tone(half_cycle: u64, seconds: f64) -> Vec<f32> {
    let mut beeper = Beeper::new(DEFAULT_SAMPLE_RATE);
    beeper.set_capacity(usize::MAX);
    let end = (CPU_HZ * seconds) as u64;
    let mut t = 0;
    let mut level = false;
    while t < end {
        beeper.write(t, level);
        level = !level;
        t += half_cycle;
    }
    beeper.advance_to(end);
    beeper.take()
}

fn sign_changes(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn the_sample_rate_does_not_drift() {
    // 3.5 MHz over 44100 Hz is 79.365 T-states a sample. Rounding that would lose a whole
    // sample every couple of hundred, and a second of sound every few minutes.
    let mut beeper = Beeper::new(DEFAULT_SAMPLE_RATE);
    beeper.set_capacity(usize::MAX);
    beeper.advance_to((CPU_HZ * 10.0) as u64);
    let count = beeper.take().len();
    // One either way: the window that is still filling when the clock stops is not
    // emitted, and floating point decides whether the last boundary lands inside.
    assert!(
        (440_999..=441_001).contains(&count),
        "ten seconds should be about 441000 samples, got {count}"
    );
}

#[test]
fn a_held_level_decays_to_silence() {
    let mut beeper = Beeper::new(DEFAULT_SAMPLE_RATE);
    beeper.set_capacity(usize::MAX);
    beeper.write(0, true); // switch the speaker on and leave it there
    beeper.advance_to((CPU_HZ * 0.5) as u64);
    let samples = beeper.take();

    // A speaker cannot hold a displacement, and neither can this: silence is silence, not
    // an offset, or every sound would begin and end with a thump.
    let tail = &samples[samples.len() / 2..];
    assert!(
        rms(tail) < 0.001,
        "a held level should decay away, rms is {}",
        rms(tail)
    );
    assert_eq!(sign_changes(&samples), 0, "and it should not oscillate");
}

#[test]
fn a_tone_comes_out_at_its_own_pitch() {
    // 3240 T-states is what Manic Miner's in-game tune uses: 540 Hz.
    let samples = tone(3240, 1.0);
    let cycles = sign_changes(&samples) as f64 / 2.0;
    assert!(
        (530.0..550.0).contains(&cycles),
        "expected about 540 cycles, counted {cycles}"
    );
    assert!(rms(&samples) > 0.1, "and it should be audible");
}

#[test]
fn a_tone_faster_than_the_sample_rate_comes_out_quiet() {
    // 40 T-states a half-cycle is 43.75 kHz, twice what 44100 Hz can carry. Integrating
    // each sample over the T-states it covers averages it away, which is what a speaker
    // would do. Point-sampling would alias it down into an audible screech instead.
    let supersonic = tone(40, 0.5);
    let audible = tone(3240, 0.5);
    assert!(
        rms(&supersonic) < rms(&audible) / 20.0,
        "supersonic rms {} should be far below audible rms {}",
        rms(&supersonic),
        rms(&audible)
    );
}

#[test]
fn a_pulse_shorter_than_a_sample_still_makes_a_sound() {
    // 200 T-states high out of every 6480 — a 540 Hz pulse train whose pulses are two and
    // a half samples wide, and which a point sampler would miss most of.
    let mut beeper = Beeper::new(DEFAULT_SAMPLE_RATE);
    beeper.set_capacity(usize::MAX);
    let mut t = 0;
    while t < CPU_HZ as u64 / 2 {
        beeper.write(t, true);
        beeper.write(t + 200, false);
        t += 6480;
    }
    beeper.advance_to(CPU_HZ as u64 / 2);
    let samples = beeper.take();
    assert!(rms(&samples) > 0.005, "rms is only {}", rms(&samples));
    // Two sign changes per pulse, over half a second.
    let hz = sign_changes(&samples) as f64 / 2.0 / 0.5;
    assert!(
        (530.0..550.0).contains(&hz),
        "the pitch should still be 540 Hz, counted {hz}"
    );
}

#[test]
fn the_wav_header_says_what_the_samples_are() {
    let file = wav(&[0.0, 0.5, -0.5], 44_100);
    assert_eq!(&file[0..4], b"RIFF");
    assert_eq!(&file[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(file[4..8].try_into().unwrap()), 36 + 6);
    assert_eq!(u16::from_le_bytes(file[22..24].try_into().unwrap()), 1); // mono
    assert_eq!(u32::from_le_bytes(file[24..28].try_into().unwrap()), 44_100);
    assert_eq!(u16::from_le_bytes(file[34..36].try_into().unwrap()), 16); // bits
    assert_eq!(file.len(), 44 + 6);
    assert_eq!(i16::from_le_bytes(file[46..48].try_into().unwrap()), 16383);
}
