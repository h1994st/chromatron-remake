//! `CHROMATRON_PROFILE=1`: per-frame timings and the launch-to-first-frame time on stderr
//! (spec §6), logged with the target `chromatron::frame_timing`.

use std::time::{Duration, Instant};

use lumen::Damage;

pub const TARGET: &str = "chromatron::frame_timing";

/// Time spent on one presented frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTimes {
    /// `GameState::handle` calls since the last frame; beams are retraced inside them.
    pub trace: Duration,
    /// `lumen::render` into the 640 × 480 frame.
    pub render: Duration,
    /// Upscaling and text compositing into the canvas.
    pub blit: Duration,
    /// Copying the canvas into the window's layer and presenting it.
    pub present: Duration,
}

/// Time spent on one presented modern frame (spec §13 "Profiling", ruling P8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModernTimes {
    /// `GameState::handle_on` calls since the last frame.
    pub trace: Duration,
    /// halation's paint (base, light, bloom and compose together: halation keeps no clock).
    pub paint: Duration,
    /// Laying out, setting and inking the captions.
    pub captions: Duration,
    /// Copying the canvas's damage into the window's layer and presenting it.
    pub present: Duration,
}

pub struct Profiler {
    enabled: bool,
    launched: Instant,
    first_frame_seen: bool,
}

impl Profiler {
    pub fn new(enabled: bool, launched: Instant) -> Profiler {
        Profiler {
            enabled,
            launched,
            first_frame_seen: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Logs one frame and what it repainted (and, the first time, how long the launch took).
    pub fn frame(&mut self, times: FrameTimes, damage: Damage) {
        if !self.enabled {
            return;
        }
        self.first_frame();
        let (kind, rects) = match damage {
            Damage::None => ("none", 0),
            Damage::Full => ("full", 0),
            Damage::Rects { len, .. } => ("partial", len),
        };
        log::info!(
            target: TARGET,
            "frame: trace {:.3} ms, render {:.3} ms, blit {:.3} ms, present {:.3} ms; damage {kind} ({rects} rects)",
            ms(times.trace),
            ms(times.render),
            ms(times.blit),
            ms(times.present),
        );
    }

    /// Logs one modern frame and how many damage rects it painted.
    pub fn modern_frame(&mut self, times: ModernTimes, rects: usize) {
        if !self.enabled {
            return;
        }
        self.first_frame();
        log::info!(
            target: TARGET,
            "modern frame: trace {:.3} ms, paint {:.3} ms, captions {:.3} ms, present {:.3} ms; damage {rects} rects",
            ms(times.trace),
            ms(times.paint),
            ms(times.captions),
            ms(times.present),
        );
    }

    /// The launch-to-first-frame line, once.
    fn first_frame(&mut self) {
        if !self.first_frame_seen {
            self.first_frame_seen = true;
            log::info!(
                target: TARGET,
                "first frame presented {:.1} ms after launch",
                ms(self.launched.elapsed())
            );
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Frames a present probe skips before it records (the window settling in).
pub const WARM_UP: usize = 10;

/// What the shell does after a probe's present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeStep {
    /// Present again.
    Next,
    /// Switch look (once, half way, when asked), then present again.
    SwitchLook,
    /// Print the summary and quit.
    Done,
}

/// A present-time distribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spread {
    pub p50: Duration,
    pub p90: Duration,
    pub max: Duration,
}

impl Spread {
    pub fn of(samples: &[Duration]) -> Spread {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Spread {
            p50: percentile(&sorted, 50.0),
            p90: percentile(&sorted, 90.0),
            max: sorted.last().copied().unwrap_or_default(),
        }
    }
}

/// The present's distribution over a probe run (ruling P8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeSummary {
    /// Paints recorded, whether or not they presented.
    pub frames: usize,
    /// Of those, the ones that presented (a damage probe's paint with no damage does not).
    pub presented: usize,
    /// The presents before the look switch (all of them without one).
    pub present: Spread,
    /// The presents after the switch, reported apart: they are the other look's (and, under
    /// softbuffer, were nearly free for tens of frames after a reshape).
    pub after_switch: Option<Spread>,
    /// The time between consecutive presents: the run's real pacing.
    pub gap_p50: Duration,
    pub gap_p90: Duration,
}

/// The probe's warnings, when its numbers cannot be trusted: the window was `hidden`
/// (occluded or invisible at some point), or nothing was `presented`. The present's own time
/// is not judged: the layer presenter's copy costs the same on screen or off.
pub fn probe_warnings(hidden: bool, presented: usize) -> [Option<&'static str>; 2] {
    [
        hidden.then_some("the window may have been hidden; rerun with it in front"),
        (presented == 0).then_some("the probe presented nothing"),
    ]
}

/// `CHROMATRON_PRESENT_PROBE=N`: N paints after `WARM_UP`, each presenting the whole canvas
/// (or, with `CHROMATRON_PRESENT_PROBE_DAMAGE`, the stage's damage); with
/// `CHROMATRON_PRESENT_PROBE_SWITCH`, a look switch half way.
pub struct PresentProbe {
    want: usize,
    seen: usize,
    /// Paints recorded after the warm-up.
    frames: usize,
    switch: bool,
    /// How many presents were recorded before the switch, once it happened.
    switched_at: Option<usize>,
    samples: Vec<Duration>,
    gaps: Vec<Duration>,
}

impl PresentProbe {
    pub fn new(frames: usize, switch: bool) -> PresentProbe {
        PresentProbe {
            want: frames,
            seen: 0,
            frames: 0,
            switch,
            switched_at: None,
            samples: Vec::with_capacity(frames),
            gaps: Vec::with_capacity(frames),
        }
    }

    /// Records the time since the previous present; call before `record`, with the
    /// previous present's gap to this one. Warm-up frames are skipped.
    pub fn record_gap(&mut self, gap: Duration) {
        if self.seen >= WARM_UP {
            self.gaps.push(gap);
        }
    }

    /// Records one paint's present (`None` when it presented nothing) and says what comes
    /// next.
    pub fn record(&mut self, present: Option<Duration>) -> ProbeStep {
        self.seen += 1;
        if self.seen > WARM_UP {
            self.frames += 1;
            self.samples.extend(present);
        }
        if self.frames >= self.want {
            ProbeStep::Done
        } else if self.switch
            && self.switched_at.is_none()
            && self.frames == self.want / 2
            && self.seen > WARM_UP
        {
            self.switched_at = Some(self.samples.len());
            ProbeStep::SwitchLook
        } else {
            ProbeStep::Next
        }
    }

    pub fn summary(&self) -> ProbeSummary {
        let (before, after) = self
            .samples
            .split_at(self.switched_at.unwrap_or(self.samples.len()));
        let mut gaps = self.gaps.clone();
        gaps.sort_unstable();
        ProbeSummary {
            frames: self.frames,
            presented: self.samples.len(),
            present: Spread::of(before),
            after_switch: self.switched_at.map(|_| Spread::of(after)),
            gap_p50: percentile(&gaps, 50.0),
            gap_p90: percentile(&gaps, 90.0),
        }
    }
}

/// The nearest-rank `p`th percentile of `sorted`; zero when empty.
pub fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

/// The probe's frame count: a whole number from 1.
pub fn parse_probe(value: &str) -> Result<usize, String> {
    match value.trim().parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!(
            "CHROMATRON_PRESENT_PROBE {value:?}: expected a frame count from 1"
        )),
    }
}

/// The largest side `CHROMATRON_PROBE_SIZE` accepts (pt).
pub const PROBE_SIDE_MAX: f64 = 8192.0;

/// `CHROMATRON_PROBE_SIZE=WxH`: a modern probe window's content size (pt) instead of the
/// default, from the modern minimum to `PROBE_SIDE_MAX` a side; unset or empty keeps the
/// default. AppKit may still fit the window to the screen; the probe prints the real size.
pub fn parse_probe_size(value: Option<&str>) -> Result<Option<(f64, f64)>, String> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let (min_w, min_h) = crate::window_shape::MODERN_MIN_PT;
    let side = |s: &str, min: f64| {
        s.parse::<u32>()
            .ok()
            .map(f64::from)
            .filter(|&v| (min..=PROBE_SIDE_MAX).contains(&v))
    };
    value
        .split_once('x')
        .and_then(|(w, h)| Some((side(w, min_w)?, side(h, min_h)?)))
        .map(Some)
        .ok_or_else(|| {
            format!(
                "CHROMATRON_PROBE_SIZE {value:?}: expected WxH in pt, from {min_w}x{min_h} to {PROBE_SIDE_MAX}x{PROBE_SIDE_MAX}"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_sizes_parse_within_bounds() {
        assert_eq!(parse_probe_size(None), Ok(None));
        assert_eq!(parse_probe_size(Some("")), Ok(None));
        assert_eq!(
            parse_probe_size(Some("2560x1400")),
            Ok(Some((2560.0, 1400.0)))
        );
        assert_eq!(parse_probe_size(Some("800x600")), Ok(Some((800.0, 600.0))));
        for bad in [
            "799x600", "800x599", "8193x800", "1280", "1280x", "x800", "1.5x800", "ax b",
        ] {
            assert!(parse_probe_size(Some(bad)).is_err(), "{bad}");
        }
    }

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let sorted: Vec<Duration> = (1..=10).map(ms).collect();
        assert_eq!(percentile(&sorted, 50.0), ms(5));
        assert_eq!(percentile(&sorted, 90.0), ms(9));
        assert_eq!(percentile(&sorted, 100.0), ms(10));
        assert_eq!(percentile(&[], 90.0), Duration::ZERO);
    }

    #[test]
    fn the_probe_skips_its_warm_up_and_stops_at_its_count() {
        let mut probe = PresentProbe::new(3, false);
        for _ in 0..WARM_UP {
            assert_eq!(probe.record(Some(ms(100))), ProbeStep::Next, "warm-up");
        }
        assert_eq!(probe.record(Some(ms(3))), ProbeStep::Next);
        assert_eq!(probe.record(Some(ms(1))), ProbeStep::Next);
        assert_eq!(probe.record(Some(ms(2))), ProbeStep::Done);
        let s = probe.summary();
        let p = s.present;
        assert_eq!((s.frames, p.p50, p.p90, p.max), (3, ms(2), ms(3), ms(3)));
        assert_eq!(s.presented, 3);
        assert_eq!(s.after_switch, None);
    }

    /// A damage probe's paint with nothing to present counts as a frame, not a sample.
    #[test]
    fn frames_that_present_nothing_count_but_are_not_sampled() {
        let mut probe = PresentProbe::new(3, false);
        for _ in 0..WARM_UP {
            probe.record(None);
        }
        assert_eq!(probe.record(Some(ms(2))), ProbeStep::Next);
        assert_eq!(probe.record(None), ProbeStep::Next);
        assert_eq!(probe.record(Some(ms(4))), ProbeStep::Done);
        let s = probe.summary();
        assert_eq!((s.frames, s.presented), (3, 2));
        assert_eq!((s.present.p50, s.present.max), (ms(2), ms(4)));
    }

    /// The presents after a switch are the other look's: they are reported apart.
    #[test]
    fn a_switching_probe_reports_the_switch_apart() {
        let mut probe = PresentProbe::new(4, true);
        for _ in 0..WARM_UP {
            probe.record(Some(ms(100)));
        }
        for present in [ms(2), ms(3)] {
            probe.record(Some(present));
        }
        let tiny = Duration::from_micros(30);
        for _ in 0..2 {
            probe.record(Some(tiny));
        }
        let s = probe.summary();
        assert_eq!(s.frames, 4);
        assert_eq!((s.present.p50, s.present.max), (ms(2), ms(3)));
        let after = s.after_switch.expect("switched");
        assert_eq!((after.p50, after.max), (tiny, tiny));
    }

    #[test]
    fn a_switching_probe_switches_once_at_half_way() {
        let mut probe = PresentProbe::new(4, true);
        let steps: Vec<ProbeStep> = (0..WARM_UP + 4)
            .map(|_| probe.record(Some(ms(1))))
            .collect();
        let switches = steps
            .iter()
            .filter(|&&s| s == ProbeStep::SwitchLook)
            .count();
        assert_eq!(switches, 1);
        assert_eq!(
            steps[WARM_UP + 1],
            ProbeStep::SwitchLook,
            "after 2 of 4 recorded"
        );
        assert_eq!(*steps.last().unwrap(), ProbeStep::Done);
    }

    #[test]
    fn gaps_skip_the_warm_up_and_have_their_own_percentiles() {
        let mut probe = PresentProbe::new(2, false);
        for _ in 0..WARM_UP {
            probe.record_gap(ms(500));
            probe.record(Some(ms(1)));
        }
        for gap in [ms(4), ms(8)] {
            probe.record_gap(gap);
            probe.record(Some(ms(1)));
        }
        let s = probe.summary();
        assert_eq!((s.gap_p50, s.gap_p90), (ms(4), ms(8)));
    }

    /// Review M2: a hidden window (occluded or invisible) and a run that presented nothing are
    /// warned about apart; how long a present took is not judged (the copy costs the same on
    /// screen or off).
    #[test]
    fn a_hidden_window_and_a_run_that_presented_nothing_are_warned_about() {
        let warned = |hidden, presented| probe_warnings(hidden, presented).iter().flatten().count();
        assert_eq!(warned(false, 1), 0);
        assert_eq!(warned(true, 240), 1);
        assert_eq!(warned(false, 0), 1);
        assert_eq!(warned(true, 0), 2);
        assert_ne!(probe_warnings(true, 0)[0], probe_warnings(true, 0)[1]);
    }

    #[test]
    fn a_disabled_profiler_logs_nothing_and_never_marks_the_first_frame() {
        let mut p = Profiler::new(false, std::time::Instant::now());
        p.modern_frame(ModernTimes::default(), 3);
        assert!(!p.enabled());
    }

    #[test]
    fn probe_counts_parse() {
        assert_eq!(parse_probe("120"), Ok(120));
        assert!(parse_probe("0").is_err());
        assert!(parse_probe("x").is_err());
    }
}
