//! Bar heights, the noise gate and beat detection, through the public
//! `undavi::spectrum` API.

use undavi::settings::{AdaptSpeed, GATE_OFF, GlowSource, SilenceMode, TuningSettings};
use undavi::spectrum::*;

fn flat_frame(db: f32) -> Vec<f32> {
    vec![db; 2049]
}

fn frame_with(mags: &[f32]) -> SpectrumFrame<'_> {
    SpectrumFrame {
        magnitudes_db: mags,
        sample_rate: 48_000.0,
        fft_size: 4096,
    }
}

/// A quiet spectrum with an optional bass and/or snare burst.
fn spectrum(bass_db: f32, snare_db: f32) -> Vec<f32> {
    (0..=2048)
        .map(|k| {
            let f = k as f32 * 48_000.0 / 4096.0;
            if (40.0..150.0).contains(&f) {
                bass_db
            } else if (1500.0..6000.0).contains(&f) {
                snare_db
            } else {
                -60.0
            }
        })
        .collect()
}

fn run(p: &mut Processor, frames: &[SpectrumFrame<'_>], t: &TuningSettings, n: usize) {
    for _ in 0..n {
        p.update(frames, 8, t, 1.0 / 60.0);
    }
}

fn flat_tuning() -> TuningSettings {
    TuningSettings {
        spread: 0,
        bass_db: 0,
        highs_db: 0,
        ..Default::default()
    }
}

#[test]
fn gate_opens_closes_and_holds() {
    let mut g = Gate::default();
    let dt = 1.0 / 60.0;
    assert!(!g.update(Some(-80.0), -70, dt), "hiss stays below");
    assert!(g.update(Some(-40.0), -70, dt));
    // Just under the threshold is within the hysteresis: still open.
    for _ in 0..60 {
        assert!(g.update(Some(-71.0), -70, dt));
    }
    // Well under it closes, but only after the hold time.
    assert!(g.update(Some(-90.0), -70, dt));
    for _ in 0..20 {
        g.update(Some(-90.0), -70, dt);
    }
    assert!(!g.is_open());
    // With the gate off, only digital silence (or no audio) counts as silence.
    assert!(g.update(Some(-95.0), GATE_OFF, dt));
    assert!(!g.update(None, GATE_OFF, dt));
}

#[test]
fn silence_fades_idles_or_freezes() {
    let loud = flat_frame(-20.0);
    let frame = [frame_with(&loud)];
    let mut tuning = flat_tuning();
    let tallest = |p: &Processor| p.visible().iter().copied().fold(0.0, f32::max);

    let mut p = Processor::default();
    run(&mut p, &frame, &tuning, 60);
    run(&mut p, &[], &tuning, 120);
    assert!(tallest(&p) < 0.01, "fade: flat");

    tuning.when_silent = SilenceMode::Wave;
    let mut p = Processor::default();
    run(&mut p, &frame, &tuning, 60);
    run(&mut p, &[], &tuning, 120);
    let before = p.visible().to_vec();
    assert!(
        (0.02..=IDLE_LEVEL).contains(&tallest(&p)),
        "idle: a low wave"
    );
    run(&mut p, &[], &tuning, 30);
    assert_ne!(before, p.visible(), "idle: it moves");

    tuning.when_silent = SilenceMode::Freeze;
    let mut p = Processor::default();
    run(&mut p, &frame, &tuning, 60);
    let playing = p.visible().to_vec();
    run(&mut p, &[], &tuning, 120);
    assert_eq!(playing, p.visible(), "freeze: bars stay put");
}

#[test]
fn gravity_falls_slow_then_fast() {
    let loud = flat_frame(-4.0);
    let frame = [frame_with(&loud)];
    let tuning = TuningSettings {
        gravity: true,
        ..flat_tuning()
    };
    let mut p = Processor::default();
    run(&mut p, &frame, &tuning, 60);
    let top = p.visible()[0];
    let mut drops = Vec::new();
    let mut last = top;
    for _ in 0..12 {
        p.update(&[], 8, &tuning, 1.0 / 60.0);
        drops.push(last - p.visible()[0]);
        last = p.visible()[0];
    }
    assert!(drops[0] < 0.01, "starts slowly: {drops:?}");
    assert!(
        drops.windows(2).all(|w| w[1] > w[0]),
        "speeds up: {drops:?}"
    );
    run(&mut p, &[], &tuning, 120);
    assert_eq!(p.visible()[0], 0.0, "and lands");
}

#[test]
fn stronger_gravity_drops_sooner() {
    let loud = flat_frame(-4.0);
    let frame = [frame_with(&loud)];
    let landing_frames = |strength: u8| {
        let tuning = TuningSettings {
            gravity: true,
            gravity_strength: strength,
            ..flat_tuning()
        };
        let mut p = Processor::default();
        run(&mut p, &frame, &tuning, 60);
        (1..600)
            .find(|_| {
                p.update(&[], 8, &tuning, 1.0 / 60.0);
                p.visible()[0] == 0.0
            })
            .expect("lands")
    };
    let (weak, normal, strong) = (landing_frames(50), landing_frames(100), landing_frames(200));
    assert!(weak > normal && normal > strong, "{weak} {normal} {strong}");
}

#[test]
fn auto_gain_holds_through_silence() {
    let quiet = flat_frame(-60.0);
    let frame = [frame_with(&quiet)];
    let tuning = TuningSettings {
        auto_gain: true,
        agc_speed: AdaptSpeed::Fast,
        ..flat_tuning()
    };
    let mut p = Processor::default();
    run(&mut p, &frame, &tuning, 120);
    let gain = p.gain();
    assert!(gain > 1.5, "{gain}");
    run(&mut p, &[], &tuning, 600);
    assert_eq!(
        p.gain(),
        gain,
        "ten seconds of silence don't push it to the max"
    );
}

#[test]
fn beat_sensitivity_sets_the_bar_for_a_hit() {
    // A low end that flutters by half a dB: about twice its average flux on
    // every rising frame. Sensitive settings hear beats in it, strict ones don't.
    let a = spectrum(-50.0, -50.0);
    let b = spectrum(-49.5, -50.0);
    let strongest = |sensitivity| {
        let mut g = BeatGlow::default();
        let mut max = 0.0f32;
        for i in 0..240 {
            let frame = frame_with(if i % 2 == 0 { &a } else { &b });
            let pulse = g.update(Some(&frame), GlowSource::Drums, sensitivity, 1.0 / 60.0);
            if i >= 120 {
                max = max.max(pulse);
            }
        }
        max
    };
    assert!(strongest(100) > 0.5, "{}", strongest(100));
    assert!(strongest(0) < 0.05, "{}", strongest(0));
}

#[test]
fn bass_glow_follows_the_low_end() {
    let mut g = BeatGlow::default();
    let quiet = spectrum(-40.0, -60.0);
    let loud = spectrum(-12.0, -60.0);
    for _ in 0..120 {
        g.update(Some(&frame_with(&quiet)), GlowSource::Bass, 50, 1.0 / 60.0);
    }
    assert!(g.pulse() < 0.1, "{}", g.pulse());
    for _ in 0..6 {
        g.update(Some(&frame_with(&loud)), GlowSource::Bass, 50, 1.0 / 60.0);
    }
    assert!(g.pulse() > 0.6, "{}", g.pulse());
}

#[test]
fn drum_hits_flash_then_fade() {
    let mut g = BeatGlow::default();
    let quiet = spectrum(-50.0, -50.0);
    let hit = spectrum(-10.0, -20.0);
    for _ in 0..60 {
        g.update(Some(&frame_with(&quiet)), GlowSource::Drums, 50, 1.0 / 60.0);
    }
    assert!(g.pulse() < 0.05);
    g.update(Some(&frame_with(&hit)), GlowSource::Drums, 50, 1.0 / 60.0);
    assert!(g.pulse() > 0.6, "{}", g.pulse());
    // Holding the same level isn't a new hit: the glow fades.
    for _ in 0..30 {
        g.update(Some(&frame_with(&hit)), GlowSource::Drums, 50, 1.0 / 60.0);
    }
    assert!(g.pulse() < 0.1, "{}", g.pulse());
}

#[test]
fn stereo_channels_stay_apart() {
    // Loud left, silent right, heavy spread: nothing bleeds across.
    let loud = vec![-10.0; 2049];
    let silent = vec![-120.0; 2049];
    let frames = [frame_with(&loud), frame_with(&silent)];
    let tuning = TuningSettings {
        spread: 100,
        auto_gain: false,
        ..Default::default()
    };
    let mut p = Processor::default();
    for _ in 0..60 {
        p.update(&frames, 8, &tuning, 1.0 / 60.0);
    }
    let (left, right) = p.visible().split_at(4);
    assert!(left.iter().all(|&v| v > 0.3), "{left:?}");
    assert!(right.iter().all(|&v| v < 0.01), "{right:?}");
}

#[test]
fn tuning_floor_and_contrast() {
    // −40 dB sits halfway between a −80 floor and 0 ceiling; contrast 100 squares it.
    let mags = vec![-40.0; 2049];
    let frame = frame_with(&mags);
    let base = TuningSettings {
        bass_db: 0,
        highs_db: 0,
        spread: 0,
        sensitivity: 100,
        rise_ms: 5,
        ..Default::default()
    };
    let level = |t: &TuningSettings| {
        let mut p = Processor::default();
        for _ in 0..60 {
            p.update(std::slice::from_ref(&frame), 4, t, 1.0 / 60.0);
        }
        p.visible()[0]
    };
    assert!((level(&base) - 0.5).abs() < 0.01);
    assert!(
        (level(&TuningSettings {
            contrast: 100,
            ..base.clone()
        }) - 0.25)
            .abs()
            < 0.01
    );
    assert!(
        (level(&TuningSettings {
            floor_db: -60,
            ceiling_db: -20,
            ..base.clone()
        }) - 0.5)
            .abs()
            < 0.01
    );
    assert!(
        (level(&TuningSettings {
            floor_db: -60,
            ..base.clone()
        }) - 1.0 / 3.0)
            .abs()
            < 0.01
    );
}

#[test]
fn eq_peaks_at_band_center() {
    let g = eq_gain_db(0.08, [4.0, 0.0, -2.0]);
    assert!((3.0..=4.0).contains(&g), "{g}");
    let g = eq_gain_db(0.85, [4.0, 0.0, -2.0]);
    assert!((-2.0..-1.5).contains(&g), "{g}");
    assert_eq!(eq_gain_db(0.5, [0.0; 3]), 0.0);
}

#[test]
fn rises_and_falls() {
    let mut p = Processor::default();
    let tuning = TuningSettings {
        bass_db: 0,
        highs_db: 0,
        spread: 0,
        ..Default::default()
    };
    let loud = flat_frame(-20.0);
    let frame = SpectrumFrame {
        magnitudes_db: &loud,
        sample_rate: 48_000.0,
        fft_size: 4096,
    };
    for _ in 0..60 {
        p.update(std::slice::from_ref(&frame), 8, &tuning, 1.0 / 60.0);
    }
    let expected = 0.75 * sensitivity_gain(72);
    for &h in p.visible() {
        assert!((h - expected).abs() < 0.01, "{h} vs {expected}");
    }
    for _ in 0..120 {
        p.update(&[], 8, &tuning, 1.0 / 60.0);
    }
    assert!(p.visible().iter().all(|&h| h < 0.01));
}

#[test]
fn auto_gain_lifts_quiet_input() {
    let mut p = Processor::default();
    let tuning = TuningSettings {
        auto_gain: true,
        spread: 0,
        bass_db: 0,
        highs_db: 0,
        ..Default::default()
    };
    let quiet = flat_frame(-60.0);
    let frame = SpectrumFrame {
        magnitudes_db: &quiet,
        sample_rate: 48_000.0,
        fft_size: 4096,
    };
    for _ in 0..120 {
        p.update(std::slice::from_ref(&frame), 8, &tuning, 1.0 / 60.0);
    }
    let tallest = p.visible().iter().copied().fold(0.0, f32::max);
    assert!((tallest - AGC_TARGET).abs() < 0.02, "{tallest}");
}

#[test]
fn calibration_targets_80_percent() {
    let mut p = Processor::default();
    let tuning = TuningSettings {
        spread: 0,
        bass_db: 0,
        highs_db: 0,
        ..Default::default()
    };
    let mags = flat_frame(-4.0); // level 0.95 before sensitivity
    let frame = SpectrumFrame {
        magnitudes_db: &mags,
        sample_rate: 48_000.0,
        fft_size: 4096,
    };
    p.start_calibration();
    let mut result = None;
    for _ in 0..400 {
        if let Some(s) = p.update(std::slice::from_ref(&frame), 8, &tuning, 1.0 / 60.0) {
            result = Some(s);
            break;
        }
    }
    let s = result.expect("calibration should finish");
    let landed = 0.95 * sensitivity_gain(s);
    assert!((landed - 0.8).abs() < 0.01, "{landed}");
}
