//! Page geometry: pointer positions in frame pixels, and the game's display scale.

use lumen::{FRAME_HEIGHT, FRAME_WIDTH};
use wasm_bindgen::prelude::*;

/// A pointer position in frame pixels (fractional, as the page computes it) as the pixel it falls
/// in. Floors, so positions left of or above the frame are negative; clamped to one frame beyond
/// each edge like the app's `Viewport::to_frame` (x in −640..=1280, y in −480..=960); NaN gives 0.
pub fn frame_point(x: f64, y: f64) -> (i32, i32) {
    let axis = |v: f64, size: usize| {
        let size = size as f64;
        v.floor().clamp(-size, 2.0 * size) as i32
    };
    (axis(x, FRAME_WIDTH), axis(y, FRAME_HEIGHT))
}

/// CSS pixels per frame pixel for a `width × height` CSS-pixel area: the largest whole multiple of
/// 640 × 480 that fits, or the exact fit when even 1× does not (phones). 1 for an empty, negative
/// or non-finite area, so the game never disappears.
#[wasm_bindgen]
pub fn display_scale(width: f64, height: f64) -> f64 {
    let fit = (width / FRAME_WIDTH as f64).min(height / FRAME_HEIGHT as f64);
    if fit.is_finite() && fit >= 1.0 {
        fit.floor()
    } else if fit > 0.0 && fit < 1.0 {
        fit
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_positions_floor_to_the_pixel_they_fall_in() {
        assert_eq!(frame_point(0.0, 0.0), (0, 0));
        assert_eq!(frame_point(639.99, 479.5), (639, 479));
        assert_eq!(frame_point(-0.25, -0.25), (-1, -1));
        assert_eq!(
            frame_point(1e9, -1e9),
            (1280, -480),
            "one frame beyond each edge"
        );
        assert_eq!(frame_point(f64::NAN, f64::INFINITY), (0, 960));
    }

    #[test]
    fn the_game_is_shown_at_the_largest_whole_scale_that_fits() {
        assert_eq!(display_scale(640.0, 480.0), 1.0);
        assert_eq!(display_scale(1919.0, 1440.0), 2.0);
        assert_eq!(display_scale(2560.0, 1440.0), 3.0);
        assert_eq!(display_scale(1300.0, 10_000.0), 2.0);
    }

    #[test]
    fn a_small_screen_gets_the_exact_fit() {
        assert_eq!(display_scale(320.0, 480.0), 0.5);
        assert_eq!(display_scale(639.0, 2000.0), 639.0 / 640.0);
    }

    #[test]
    fn an_empty_or_nonsensical_area_gets_1x() {
        assert_eq!(display_scale(0.0, 0.0), 1.0);
        assert_eq!(display_scale(-5.0, 100.0), 1.0);
        assert_eq!(display_scale(f64::NAN, f64::NAN), 1.0);
        assert_eq!(display_scale(f64::INFINITY, f64::INFINITY), 1.0);
    }
}
