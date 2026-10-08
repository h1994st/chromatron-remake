//! Integer scaling of the 640 × 480 frame into the window's device pixels, centred with
//! letterbox bars (spec §5: integer scale, nearest neighbour).

use lumen::{FRAME_HEIGHT, FRAME_WIDTH, Rect};

/// The View ▸ Scale menu's range: device pixels per frame pixel.
pub const MIN_SCALE: u32 = 1;
pub const MAX_SCALE: u32 = 4;

/// A rectangle in device pixels, `[left, right) × [top, bottom)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceRect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

/// Where the frame lands in a surface of `width × height` device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub width: usize,
    pub height: usize,
    /// Device pixels per frame pixel, at least 1.
    pub scale: usize,
    /// Device position of the frame's top-left pixel.
    pub left: usize,
    pub top: usize,
}

impl Viewport {
    /// The largest integer scale that fits, centred. A surface smaller than the frame gets scale 1
    /// at the top-left and the frame is clipped.
    pub fn fit(width: usize, height: usize) -> Viewport {
        let scale = (width / FRAME_WIDTH).min(height / FRAME_HEIGHT).max(1);
        Viewport {
            width,
            height,
            scale,
            left: width.saturating_sub(FRAME_WIDTH * scale) / 2,
            top: height.saturating_sub(FRAME_HEIGHT * scale) / 2,
        }
    }

    /// A device position (winit's physical cursor position) in frame pixels. Floors, so points
    /// left of or above the frame are negative and every frame pixel owns `scale × scale` device
    /// pixels. Clamped to one frame beyond each edge (x in −640..=1280, y in −480..=960), which
    /// is far enough for any drag and keeps huge or non-finite positions sane (NaN gives 0).
    pub fn to_frame(&self, x: f64, y: f64) -> (i32, i32) {
        let scale = self.scale as f64;
        let axis = |v: f64, origin: usize, size: usize| {
            let size = size as f64;
            ((v - origin as f64) / scale)
                .floor()
                .clamp(-size, 2.0 * size) as i32
        };
        (
            axis(x, self.left, FRAME_WIDTH),
            axis(y, self.top, FRAME_HEIGHT),
        )
    }

    /// `rect` (frame pixels) clipped to the frame, in device pixels clipped to the surface.
    /// `None` when nothing is left.
    pub fn device_rect(&self, rect: Rect) -> Option<DeviceRect> {
        let left = rect.left.max(0) as usize;
        let top = rect.top.max(0) as usize;
        let right = rect.right.clamp(0, FRAME_WIDTH as i32) as usize;
        let bottom = rect.bottom.clamp(0, FRAME_HEIGHT as i32) as usize;
        let device = DeviceRect {
            left: (self.left + left * self.scale).min(self.width),
            top: (self.top + top * self.scale).min(self.height),
            right: (self.left + right * self.scale).min(self.width),
            bottom: (self.top + bottom * self.scale).min(self.height),
        };
        (device.left < device.right && device.top < device.bottom).then_some(device)
    }
}

/// The window's content size in device pixels for View ▸ Scale `scale`.
pub fn window_size(scale: u32) -> (u32, u32) {
    (FRAME_WIDTH as u32 * scale, FRAME_HEIGHT as u32 * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_multiples_have_no_bars() {
        for scale in 1..=4 {
            let vp = Viewport::fit(640 * scale, 480 * scale);
            assert_eq!((vp.scale, vp.left, vp.top), (scale, 0, 0));
        }
    }

    #[test]
    fn fullscreen_picks_the_largest_fit_and_centres_it() {
        // A 14" MacBook Pro in fullscreen: 3024 × 1964 device pixels.
        let vp = Viewport::fit(3024, 1964);
        assert_eq!(vp.scale, 4);
        assert_eq!((vp.left, vp.top), ((3024 - 2560) / 2, (1964 - 1920) / 2));
        // A 5K display: 5120 × 2880 → 6× (3840 × 2880).
        let vp = Viewport::fit(5120, 2880);
        assert_eq!((vp.scale, vp.left, vp.top), (6, 640, 0));
    }

    #[test]
    fn a_surface_smaller_than_the_frame_uses_scale_1() {
        let vp = Viewport::fit(600, 400);
        assert_eq!((vp.scale, vp.left, vp.top), (1, 0, 0));
    }

    #[test]
    fn device_positions_map_to_frame_pixels_by_flooring() {
        let vp = Viewport::fit(1280 + 20, 960);
        assert_eq!(vp.left, 10);
        assert_eq!(vp.to_frame(10.0, 0.0), (0, 0));
        assert_eq!(vp.to_frame(11.9, 1.9), (0, 0));
        assert_eq!(vp.to_frame(12.0, 2.0), (1, 1));
        assert_eq!(vp.to_frame(9.5, -0.5), (-1, -1));
        assert_eq!(vp.to_frame(10.0 + 1279.0, 959.0), (639, 479));
        assert_eq!(vp.to_frame(10.0 + 1280.0, 960.0), (640, 480));
        assert_eq!(vp.to_frame(-1e12, 1e12), (-640, 960));
        assert_eq!(vp.to_frame(f64::NEG_INFINITY, f64::NAN), (-640, 0));
    }

    #[test]
    fn device_rects_are_clipped_to_the_frame_and_the_surface() {
        let vp = Viewport::fit(1280, 960);
        assert_eq!(
            vp.device_rect(Rect::new(10, 20, 30, 40)),
            Some(DeviceRect {
                left: 20,
                top: 40,
                right: 60,
                bottom: 80
            })
        );
        assert_eq!(
            vp.device_rect(Rect::new(-30, -30, 30, 30)),
            Some(DeviceRect {
                left: 0,
                top: 0,
                right: 60,
                bottom: 60
            })
        );
        assert_eq!(
            vp.device_rect(Rect::new(630, 470, 700, 500))
                .map(|r| (r.right, r.bottom)),
            Some((1280, 960))
        );
        assert_eq!(vp.device_rect(Rect::new(-50, -50, -10, -10)), None);
        assert_eq!(vp.device_rect(Rect::new(700, 0, 760, 60)), None);
        let small = Viewport::fit(600, 400);
        assert_eq!(small.device_rect(Rect::new(620, 0, 640, 10)), None);
    }

    #[test]
    fn window_sizes_are_whole_frames() {
        assert_eq!(window_size(1), (640, 480));
        assert_eq!(window_size(3), (1920, 1440));
    }
}
