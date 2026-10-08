//! Turns modern scroll events into the original's one-level-per-event wheel (platform.md P4).
//!
//! A notched mouse wheel sends one event per notch, and each one steps a level, as on Mac OS X
//! 10.3. A trackpad or Magic Mouse sends a stream of small pixel deltas plus momentum events; the
//! whole stream (one gesture) steps once, after it has travelled `PIXELS_PER_STEP`. A gesture ends
//! after `QUIET` without scroll events. The delta's sign is used as macOS delivers it, so the
//! user's natural-scrolling setting is honoured: scrolling content down (delta > 0) goes to the
//! previous level. This deliberately differs from Mac OS X 10.3, which had no natural scrolling
//! and where rolling the wheel away from the user meant the previous level.

use std::time::{Duration, Instant};

/// Logical pixels a gesture must travel before it steps.
pub const PIXELS_PER_STEP: f64 = 20.0;
/// A gap this long between pixel deltas starts a new gesture.
pub const QUIET: Duration = Duration::from_millis(150);

/// One scroll event: notched lines, or precise logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    Lines { x: f64, y: f64 },
    Pixels { x: f64, y: f64 },
}

#[derive(Debug, Default)]
pub struct WheelGate {
    last: Option<Instant>,
    travel: f64,
    stepped: bool,
}

impl WheelGate {
    /// The `InputEvent::Wheel` delta to send for this event, if any: `1` (previous level) or
    /// `-1` (next level). The axis isn't read by the original, so the larger one counts.
    pub fn feed(&mut self, delta: ScrollDelta, at: Instant) -> Option<i32> {
        match delta {
            ScrollDelta::Lines { x, y } => {
                self.last = None;
                sign(dominant(x, y))
            }
            ScrollDelta::Pixels { x, y } => {
                if self
                    .last
                    .is_none_or(|last| at.duration_since(last) >= QUIET)
                {
                    self.travel = 0.0;
                    self.stepped = false;
                }
                self.last = Some(at);
                if self.stepped {
                    return None;
                }
                self.travel += dominant(x, y);
                if self.travel.abs() < PIXELS_PER_STEP {
                    return None;
                }
                self.stepped = true;
                sign(self.travel)
            }
        }
    }
}

fn dominant(x: f64, y: f64) -> f64 {
    if y.abs() >= x.abs() { y } else { x }
}

fn sign(v: f64) -> Option<i32> {
    if v > 0.0 {
        Some(1)
    } else if v < 0.0 {
        Some(-1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn every_wheel_notch_steps_once() {
        let mut gate = WheelGate::default();
        let t = Instant::now();
        assert_eq!(gate.feed(ScrollDelta::Lines { x: 0.0, y: 1.0 }, t), Some(1));
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: 3.0 }, t + ms(5)),
            Some(1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: -1.0 }, t + ms(9)),
            Some(-1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: 0.0 }, t + ms(12)),
            None
        );
    }

    #[test]
    fn a_trackpad_gesture_with_momentum_steps_once() {
        let mut gate = WheelGate::default();
        let t = Instant::now();
        let steps: Vec<i32> = (0..120)
            .filter_map(|i| gate.feed(ScrollDelta::Pixels { x: 0.0, y: -6.0 }, t + ms(8 * i)))
            .collect();
        assert_eq!(steps, [-1]);
    }

    #[test]
    fn a_pause_starts_a_new_gesture() {
        let mut gate = WheelGate::default();
        let t = Instant::now();
        let pixels = ScrollDelta::Pixels { x: 0.0, y: 25.0 };
        assert_eq!(gate.feed(pixels, t), Some(1));
        assert_eq!(gate.feed(pixels, t + ms(100)), None);
        assert_eq!(gate.feed(pixels, t + ms(100) + QUIET), Some(1));
    }

    #[test]
    fn a_small_nudge_does_nothing() {
        let mut gate = WheelGate::default();
        let t = Instant::now();
        for i in 0..3 {
            assert_eq!(
                gate.feed(ScrollDelta::Pixels { x: 0.0, y: 5.0 }, t + ms(10 * i)),
                None
            );
        }
        assert_eq!(
            gate.feed(ScrollDelta::Pixels { x: 0.0, y: 5.0 }, t + ms(400)),
            None
        );
    }

    #[test]
    fn the_larger_axis_decides() {
        let mut gate = WheelGate::default();
        let t = Instant::now();
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: -2.0, y: 1.0 }, t),
            Some(-1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Pixels { x: 30.0, y: -4.0 }, t + ms(1)),
            Some(1)
        );
    }
}
