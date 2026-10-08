//! Nearest-neighbour upscaling of frame rects into the device-pixel canvas.

use lumen::{FRAME_WIDTH, Rect};

use crate::viewport::Viewport;

/// Colour of the bars around the frame in fullscreen (0x00RRGGBB).
pub const LETTERBOX: u32 = 0x000000;

/// Paints everything outside the frame with [`LETTERBOX`].
pub fn fill_letterbox(canvas: &mut [u32], vp: &Viewport) {
    let frame = vp
        .device_rect(Rect::new(0, 0, i32::MAX, i32::MAX))
        .expect("the frame always covers the surface's top-left device pixel");
    for (y, row) in canvas.chunks_exact_mut(vp.width).enumerate() {
        if y < frame.top || y >= frame.bottom {
            row.fill(LETTERBOX);
        } else {
            row[..frame.left].fill(LETTERBOX);
            row[frame.right..].fill(LETTERBOX);
        }
    }
}

/// Upscales `rect` of `frame` (640 × 480, row-major) into `canvas` (`vp.width × vp.height`).
/// Each frame pixel becomes a `scale × scale` block; parts outside the frame or the surface are
/// skipped.
pub fn blit_scaled(frame: &[u32], canvas: &mut [u32], vp: &Viewport, rect: Rect) {
    let Some(device) = vp.device_rect(rect) else {
        return;
    };
    let scale = vp.scale;
    let width = vp.width;
    let first_x = (device.left - vp.left) / scale;
    let first_y = (device.top - vp.top) / scale;
    let last_y = (device.bottom - 1 - vp.top) / scale;
    for y in first_y..=last_y {
        let row_top = (vp.top + y * scale).max(device.top);
        let row_end = (vp.top + (y + 1) * scale).min(device.bottom);
        let src = &frame[y * FRAME_WIDTH + first_x..(y + 1) * FRAME_WIDTH];
        let start = row_top * width;
        let line = &mut canvas[start + device.left..start + device.right];
        // The first column may be cut by the surface edge only on the right, so chunks start
        // aligned with frame pixels.
        for (block, &pixel) in line.chunks_mut(scale).zip(src) {
            block.fill(pixel);
        }
        for dy in row_top + 1..row_end {
            canvas.copy_within(
                start + device.left..start + device.right,
                dy * width + device.left,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::FRAME_HEIGHT;

    fn numbered_frame() -> Vec<u32> {
        (0..(FRAME_WIDTH * FRAME_HEIGHT) as u32).collect()
    }

    #[test]
    fn a_full_blit_at_2x_repeats_each_pixel_in_a_2x2_block() {
        let frame = numbered_frame();
        let vp = Viewport::fit(1280, 960);
        let mut canvas = vec![u32::MAX; 1280 * 960];
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(0, 0, 640, 480));
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(canvas[dy * 1280 + dx], 0);
            assert_eq!(canvas[(2 + dy) * 1280 + 2 + dx], 641);
        }
        assert_eq!(canvas[959 * 1280 + 1279], (640 * 480 - 1) as u32);
        assert!(!canvas.contains(&u32::MAX));
    }

    #[test]
    fn a_partial_blit_touches_only_its_rect() {
        let frame = numbered_frame();
        let vp = Viewport::fit(1920, 1440);
        let mut canvas = vec![u32::MAX; 1920 * 1440];
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(10, 20, 12, 21));
        let written: Vec<usize> = (0..canvas.len())
            .filter(|&i| canvas[i] != u32::MAX)
            .collect();
        assert_eq!(written.len(), 2 * 3 * 3);
        assert_eq!(canvas[60 * 1920 + 30], (20 * 640 + 10) as u32);
        assert_eq!(canvas[62 * 1920 + 35], (20 * 640 + 11) as u32);
    }

    #[test]
    fn rects_hanging_off_the_frame_are_clipped() {
        let frame = numbered_frame();
        let vp = Viewport::fit(1280, 960);
        let mut canvas = vec![u32::MAX; 1280 * 960];
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(-30, -30, 1, 1));
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(639, 479, 700, 520));
        assert_eq!(canvas.iter().filter(|&&p| p != u32::MAX).count(), 8);
        assert_eq!(canvas[959 * 1280 + 1279], (640 * 480 - 1) as u32);
    }

    #[test]
    fn letterboxed_blits_are_offset_and_bars_are_filled() {
        let frame = numbered_frame();
        let vp = Viewport::fit(1300, 970);
        assert_eq!((vp.scale, vp.left, vp.top), (2, 10, 5));
        let mut canvas = vec![u32::MAX; 1300 * 970];
        fill_letterbox(&mut canvas, &vp);
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(0, 0, 640, 480));
        assert!(!canvas.contains(&u32::MAX));
        assert_eq!(canvas[0], LETTERBOX);
        assert_eq!(canvas[5 * 1300 + 9], LETTERBOX);
        assert_eq!(canvas[5 * 1300 + 10], 0);
        assert_eq!(canvas[964 * 1300 + 1289], (640 * 480 - 1) as u32);
        assert_eq!(canvas[965 * 1300 + 1289], LETTERBOX);
    }

    #[test]
    fn a_surface_smaller_than_the_frame_is_clipped_not_overrun() {
        let frame = numbered_frame();
        let vp = Viewport::fit(600, 400);
        let mut canvas = vec![u32::MAX; 600 * 400];
        fill_letterbox(&mut canvas, &vp);
        blit_scaled(&frame, &mut canvas, &vp, Rect::new(0, 0, 640, 480));
        assert_eq!(canvas[399 * 600 + 599], (399 * 640 + 599) as u32);
    }

    #[test]
    fn any_surface_size_is_painted_without_overrun() {
        let frame = numbered_frame();
        for (width, height) in [(1, 1), (639, 479), (641, 481), (1281, 959), (3441, 1441)] {
            let vp = Viewport::fit(width, height);
            let mut canvas = vec![u32::MAX; width * height];
            fill_letterbox(&mut canvas, &vp);
            blit_scaled(&frame, &mut canvas, &vp, Rect::new(0, 0, 640, 480));
            blit_scaled(&frame, &mut canvas, &vp, Rect::new(600, 440, 700, 520));
            assert!(!canvas.contains(&u32::MAX), "{width} × {height}");
        }
    }
}
