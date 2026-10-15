//! The 640 × 480 frame (`0x00RRGGBB`, lumen's format) as RGBA bytes for the page's `ImageData`,
//! one damaged rect at a time. No allocation.

use lumen::{FRAME_HEIGHT, FRAME_WIDTH, Rect};

/// Bytes in the page's RGBA copy of the frame.
pub const RGBA_LEN: usize = FRAME_WIDTH * FRAME_HEIGHT * 4;

/// `rect` clipped to the frame; `None` when nothing is left.
pub fn clip_to_frame(rect: Rect) -> Option<Rect> {
    let clipped = Rect::new(
        rect.left.max(0),
        rect.top.max(0),
        rect.right.min(FRAME_WIDTH as i32),
        rect.bottom.min(FRAME_HEIGHT as i32),
    );
    (clipped.left < clipped.right && clipped.top < clipped.bottom).then_some(clipped)
}

/// Copies `rect` (clipped to the frame) of `frame` (640 × 480, `0x00RRGGBB`) into `rgba`
/// (640 × 480 × 4) as `[r, g, b, 255]` per pixel and returns the clipped rect, or `None` when the
/// rect is off the frame. Allocation-free.
pub fn rgba_rect(frame: &[u32], rgba: &mut [u8], rect: Rect) -> Option<Rect> {
    // frame: 640 × 480; rgba: 640 × 480 × 4.
    assert!(frame.len() == FRAME_WIDTH * FRAME_HEIGHT && rgba.len() == RGBA_LEN);
    let clipped = clip_to_frame(rect)?;
    let (left, right) = (clipped.left as usize, clipped.right as usize);
    for y in clipped.top as usize..clipped.bottom as usize {
        let row = y * FRAME_WIDTH;
        let src = &frame[row + left..row + right];
        let (dst, _) = rgba[(row + left) * 4..(row + right) * 4].as_chunks_mut::<4>();
        for (&pixel, out) in src.iter().zip(dst) {
            *out = [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 0xff];
        }
    }
    Some(clipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::{FRAME_HEIGHT, FRAME_WIDTH};

    fn rgba_at(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * FRAME_WIDTH + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn a_pixel_becomes_red_green_blue_and_opaque() {
        let mut frame = vec![0u32; FRAME_WIDTH * FRAME_HEIGHT];
        frame[FRAME_WIDTH + 2] = 0x123456;
        let mut rgba = vec![0u8; RGBA_LEN];
        assert_eq!(
            rgba_rect(&frame, &mut rgba, Rect::new(0, 0, 640, 480)),
            Some(Rect::new(0, 0, 640, 480))
        );
        assert_eq!(rgba_at(&rgba, 2, 1), [0x12, 0x34, 0x56, 0xff]);
        assert_eq!(rgba_at(&rgba, 0, 0), [0, 0, 0, 0xff]);
    }

    #[test]
    fn only_the_rect_is_written() {
        let frame = vec![0xa4a4a4u32; FRAME_WIDTH * FRAME_HEIGHT];
        let mut rgba = vec![7u8; RGBA_LEN];
        rgba_rect(&frame, &mut rgba, Rect::new(10, 20, 12, 21));
        assert_eq!(rgba_at(&rgba, 10, 20), [0xa4, 0xa4, 0xa4, 0xff]);
        assert_eq!(rgba_at(&rgba, 11, 20), [0xa4, 0xa4, 0xa4, 0xff]);
        for (x, y) in [(9, 20), (12, 20), (10, 19), (10, 21)] {
            assert_eq!(rgba_at(&rgba, x, y), [7; 4], "({x}, {y})");
        }
    }

    #[test]
    fn rects_are_clipped_to_the_frame() {
        assert_eq!(
            clip_to_frame(Rect::new(-5, -5, 3, 2)),
            Some(Rect::new(0, 0, 3, 2))
        );
        assert_eq!(
            clip_to_frame(Rect::new(630, 470, 700, 500)),
            Some(Rect::new(630, 470, 640, 480))
        );
        assert_eq!(clip_to_frame(Rect::new(700, 0, 800, 10)), None);
        assert_eq!(clip_to_frame(Rect::new(5, 5, 5, 9)), None);
        assert_eq!(clip_to_frame(Rect::new(9, 9, 5, 5)), None);
        let frame = vec![0u32; FRAME_WIDTH * FRAME_HEIGHT];
        let mut rgba = vec![0u8; RGBA_LEN];
        assert_eq!(
            rgba_rect(&frame, &mut rgba, Rect::new(-30, -30, -1, -1)),
            None
        );
    }
}
