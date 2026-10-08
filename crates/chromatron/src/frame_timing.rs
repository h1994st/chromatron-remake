//! `CHROMATRON_PROFILE=1`: per-frame timings and the launch-to-first-frame time on stderr
//! (spec §6), logged with the target `chromatron::frame_timing`.

use std::time::{Duration, Instant};

use crate::damage::Damage;

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
    /// Copying the canvas into softbuffer and presenting it.
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
        if !self.first_frame_seen {
            self.first_frame_seen = true;
            log::info!(
                target: TARGET,
                "first frame presented {:.1} ms after launch",
                ms(self.launched.elapsed())
            );
        }
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
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
