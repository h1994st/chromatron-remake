//! When the event loop wakes (spec §13 "Paint and wake", §9.1 "Idle"): at the lightbox's next
//! frame while a cue runs, otherwise only for events. Classic never animates.

use std::time::{Duration, Instant};

use winit::event_loop::ControlFlow;

use crate::look_switch::Look;

/// The control flow after a paint at `now_ms` (ms since `clock`), given the lightbox's
/// `next_wake` on the same clock: `WaitUntil` that time (a past one means now), or `Wait` when
/// idle, in classic, or for a time that is not a finite number.
pub fn wake_flow(look: Look, next_wake: Option<f64>, now_ms: f64, clock: Instant) -> ControlFlow {
    match (look, next_wake) {
        (Look::Modern, Some(at)) if at.is_finite() => {
            let ms = at.max(now_ms).max(0.0);
            ControlFlow::WaitUntil(clock + Duration::from_secs_f64(ms / 1000.0))
        }
        _ => ControlFlow::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_flow_sleeps_in_classic_and_at_rest() {
        let clock = Instant::now();
        assert_eq!(
            wake_flow(Look::Classic, Some(1016.7), 1000.0, clock),
            ControlFlow::Wait
        );
        assert_eq!(
            wake_flow(Look::Modern, None, 1000.0, clock),
            ControlFlow::Wait
        );
    }

    #[test]
    fn wake_flow_wakes_at_a_running_cues_next_frame() {
        let clock = Instant::now();
        assert_eq!(
            wake_flow(Look::Modern, Some(1016.5), 1000.0, clock),
            ControlFlow::WaitUntil(clock + Duration::from_secs_f64(1.0165))
        );
    }

    #[test]
    fn wake_flow_never_waits_backwards_or_on_a_broken_time() {
        let clock = Instant::now();
        assert_eq!(
            wake_flow(Look::Modern, Some(10.0), 1000.0, clock),
            ControlFlow::WaitUntil(clock + Duration::from_secs(1)),
            "a past time wakes now"
        );
        for broken in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                wake_flow(Look::Modern, Some(broken), 1000.0, clock),
                ControlFlow::Wait,
                "{broken}"
            );
        }
    }
}
