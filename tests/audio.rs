//! Spectrum analysis, through the public `undavi::audio` API (no audio device needed).

use undavi::audio::{Analyzer, FFT_SIZE, Signal};

const SR: f32 = 48_000.0;

/// `frames` left/right pairs of a full-scale sine at FFT bin `bin` (for an FFT of
/// `size`), on the left only or on both sides.
fn tone(bin: usize, size: usize, frames: usize, both: bool) -> Vec<f32> {
    let f = bin as f32 * SR / size as f32;
    (0..frames)
        .flat_map(|i| {
            let s = (std::f32::consts::TAU * f * i as f32 / SR).sin();
            [s, if both { s } else { 0.0 }]
        })
        .collect()
}

#[test]
fn full_scale_sine_is_zero_db() {
    let bin = 100;
    let mut a = Analyzer::default();
    a.push(&tone(bin, FFT_SIZE, 2 * FFT_SIZE, false));
    a.compute(&[Signal::Left, Signal::Right, Signal::Side], 0);
    let (left, right, side) = (
        a.spectrum(Signal::Left),
        a.spectrum(Signal::Right),
        a.spectrum(Signal::Side),
    );
    assert!(left[bin].abs() < 0.5, "{}", left[bin]);
    assert!(right[bin] < -100.0);
    // The mix and the side each get half of it: -6 dB.
    assert!((side[bin] + 6.02).abs() < 0.5, "{}", side[bin]);
    let mix = a.spectrum(Signal::Mix);
    assert!((mix[bin] + 6.02).abs() < 0.5, "{}", mix[bin]);

    // The same tone on both sides: full level in the mix, nothing in the side.
    a.push(&tone(bin, FFT_SIZE, 2 * FFT_SIZE, true));
    a.compute(&[Signal::Side], 0);
    let mix = a.spectrum(Signal::Mix);
    assert!(mix[bin].abs() < 0.5, "{}", mix[bin]);
    assert!(mix[bin + 20] < -60.0);
    assert!(
        a.spectrum(Signal::Side)[bin] < -100.0,
        "identical sides cancel"
    );
    assert!(
        a.level_db().abs() < 0.5,
        "a full-scale sine reads 0 dB: {}",
        a.level_db()
    );
}

#[test]
fn larger_ffts_reuse_the_history() {
    let bin = 100;
    let mut a = Analyzer::default();
    a.push(&tone(bin, FFT_SIZE, 8192, true));
    // Switching to the 8192 FFT keeps the samples; the same tone lands in bin 200.
    a.set_size(8192);
    a.compute(&[], 0);
    let mix = a.spectrum(Signal::Mix);
    assert_eq!(mix.len(), 4097);
    assert!(mix[2 * bin].abs() < 0.5, "{}", mix[2 * bin]);
}

#[test]
fn delay_looks_back_in_time() {
    let mut a = Analyzer::default();
    // A loud tone, then a quarter second of silence.
    let quiet = 12_000;
    a.push(&tone(100, FFT_SIZE, 2 * FFT_SIZE, true));
    a.push(&vec![0.0; 2 * quiet]);
    a.compute(&[], 0);
    assert!(a.level_db() < -100.0, "{}", a.level_db());
    // Looking back past the silence finds the tone.
    a.compute(&[], quiet);
    assert!(a.level_db() > -6.0, "{}", a.level_db());
}
