//! Text boxes drawn with Core Text in Lucida Grande 13 pt, black, left-aligned and word-wrapped
//! to the box (platform.md P3). Each box is rasterized once per scale into an alpha mask at
//! device resolution and cached; compositing darkens the canvas by the mask.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

use lumen::Rect;
use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFRange, CFRetained, CFString, CFType, CGPoint, CGRect,
    CGSize,
};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo, CGPath};
use objc2_core_text::{CTFont, CTFramesetter, kCTFontAttributeName};

use crate::viewport::{DeviceRect, Viewport};

/// `kThemeSystemFont` on Mac OS X 10.3 (platform.md P3).
pub const FONT_NAME: &str = "LucidaGrande";
pub const FONT_POINTS: f64 = 13.0;

/// One text box's coverage, `width × height` bytes, top row first; 255 = full ink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMask {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

/// Lays out `text` in `font` in a `width × height` pixel box: left-aligned, word-wrapped, first line at the top, lines that don't fit dropped.
pub fn rasterize(font: &CTFont, text: &str, width: usize, height: usize) -> TextMask {
    let mut alpha = vec![0u8; width * height];
    if width == 0 || height == 0 {
        return TextMask {
            width,
            height,
            alpha,
        };
    }
    // SAFETY: `alpha` is `width × height` bytes and outlives the context, which is dropped at the
    // end of this function. An alpha-only context with 8 bits per component and `width` bytes per
    // row takes no colour space.
    let context = unsafe {
        CGBitmapContextCreate(
            alpha.as_mut_ptr().cast::<c_void>(),
            width,
            height,
            8,
            width,
            None,
            CGImageAlphaInfo::Only.0,
        )
    }
    .expect("Core Graphics makes an alpha-only bitmap context");
    CGContext::set_should_antialias(Some(&context), true);
    // SAFETY: `kCTFontAttributeName` is an immutable CFString constant.
    let key: &CFString = unsafe { kCTFontAttributeName };
    let attributes = CFDictionary::<CFString, CFType>::from_slices(&[key], &[font.as_ref()]);
    // SAFETY: both arguments are valid CF objects; the dictionary maps the font attribute to a
    // CTFont, as Core Text requires.
    let string = unsafe {
        CFAttributedString::new(
            None,
            Some(&CFString::from_str(text)),
            Some(attributes.as_opaque()),
        )
    }
    .expect("Core Foundation makes an attributed string");
    let bounds = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as f64, height as f64),
    );
    // SAFETY: plain value arguments; a null transform means identity.
    let path = unsafe { CGPath::with_rect(bounds, ptr::null()) };
    // SAFETY: valid attributed string; a zero-length range lays out the whole string.
    let frame = unsafe {
        CTFramesetter::with_attributed_string(&string).frame(CFRange::new(0, 0), &path, None)
    };
    // SAFETY: the context is a live bitmap context.
    unsafe { frame.draw(&context) };
    drop(context);
    TextMask {
        width,
        height,
        alpha,
    }
}

/// Lucida Grande at `points × scale` pixels. Logs a warning if the system substituted another
/// font (Core Text never fails, it falls back).
pub fn system_font(scale: usize) -> CFRetained<CTFont> {
    let name = CFString::from_str(FONT_NAME);
    // SAFETY: a null matrix means identity.
    let font = unsafe { CTFont::with_name(&name, FONT_POINTS * scale as f64, ptr::null()) };
    // SAFETY: `font` is a live CTFont.
    let actual = unsafe { font.post_script_name() }.to_string();
    if actual != FONT_NAME {
        log::warn!("{FONT_NAME} is not installed; text uses {actual}");
    }
    font
}

/// `pixel` with its colour channels scaled by `1 − alpha/255`: black ink at that coverage.
pub fn darken(pixel: u32, alpha: u8) -> u32 {
    let keep = 255 - u32::from(alpha);
    let channel = |shift: u32| ((((pixel >> shift) & 0xff) * keep + 127) / 255) << shift;
    channel(16) | channel(8) | channel(0)
}

/// Darkens `canvas` (`canvas_width` wide) by `mask` placed with its top-left at `origin`, only
/// inside `clip`.
pub fn composite(
    mask: &TextMask,
    canvas: &mut [u32],
    canvas_width: usize,
    origin: (usize, usize),
    clip: DeviceRect,
) {
    let left = origin.0.max(clip.left);
    let top = origin.1.max(clip.top);
    let right = (origin.0 + mask.width).min(clip.right);
    let bottom = (origin.1 + mask.height).min(clip.bottom);
    if left >= right || top >= bottom {
        return;
    }
    for y in top..bottom {
        let src = &mask.alpha[(y - origin.1) * mask.width + left - origin.0..][..right - left];
        let dst = &mut canvas[y * canvas_width + left..y * canvas_width + right];
        for (pixel, &alpha) in dst.iter_mut().zip(src) {
            if alpha != 0 {
                *pixel = darken(*pixel, alpha);
            }
        }
    }
}

/// Cache key: the string and its rect. The data's strings are `'static`, so a key never
/// allocates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextKey {
    pub text: &'static str,
    pub rect: [i32; 4],
}

impl TextKey {
    pub fn new(text: &'static str, rect: Rect) -> TextKey {
        TextKey {
            text,
            rect: [rect.left, rect.top, rect.right, rect.bottom],
        }
    }
}

/// Rasterized text boxes for one scale. Changing the scale empties the cache.
#[derive(Default)]
pub struct TypesetCache {
    scale: usize,
    font: Option<CFRetained<CTFont>>,
    masks: HashMap<TextKey, TextMask>,
}

impl TypesetCache {
    /// The mask for `text` in `rect` (frame pixels) at `scale` device pixels per frame pixel.
    pub fn mask(&mut self, text: &'static str, rect: Rect, scale: usize) -> &TextMask {
        if scale != self.scale {
            self.scale = scale;
            self.masks.clear();
            self.font = None;
        }
        let font = self.font.get_or_insert_with(|| system_font(scale));
        self.masks
            .entry(TextKey::new(text, rect))
            .or_insert_with(|| {
                let width = (rect.right - rect.left).max(0) as usize * scale;
                let height = (rect.bottom - rect.top).max(0) as usize * scale;
                rasterize(font, text, width, height)
            })
    }

    /// Number of cached masks.
    pub fn len(&self) -> usize {
        self.masks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }

    /// Composites every box in `boxes` over the canvas, clipped to `clip` (device pixels).
    pub fn draw(
        &mut self,
        boxes: &[lumen::TextBox<'static>],
        canvas: &mut [u32],
        vp: &Viewport,
        clip: DeviceRect,
    ) {
        for text_box in boxes {
            let origin = (
                vp.left + text_box.rect.left.max(0) as usize * vp.scale,
                vp.top + text_box.rect.top.max(0) as usize * vp.scale,
            );
            let mask = self.mask(text_box.text, text_box.rect, vp.scale);
            composite(mask, canvas, vp.width, origin, clip);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: DeviceRect = DeviceRect {
        left: 0,
        top: 0,
        right: usize::MAX,
        bottom: usize::MAX,
    };

    #[test]
    fn darken_scales_each_channel() {
        assert_eq!(darken(0xa4a4a4, 0), 0xa4a4a4);
        assert_eq!(darken(0xa4a4a4, 255), 0);
        assert_eq!(darken(0xffffff, 128), 0x7f7f7f);
        assert_eq!(darken(0xff8000, 255 / 2), 0x804000);
    }

    #[test]
    fn composite_writes_only_inside_mask_and_clip() {
        let mask = TextMask {
            width: 2,
            height: 2,
            alpha: vec![255, 0, 0, 255],
        };
        let mut canvas = vec![0xffffffu32; 16];
        composite(&mask, &mut canvas, 4, (1, 1), WIDE);
        let inked: Vec<usize> = (0..16).filter(|&i| canvas[i] == 0).collect();
        assert_eq!(inked, [5, 10]);

        let mut canvas = vec![0xffffffu32; 16];
        let clip = DeviceRect {
            left: 0,
            top: 0,
            right: 2,
            bottom: 2,
        };
        composite(&mask, &mut canvas, 4, (1, 1), clip);
        let inked: Vec<usize> = (0..16).filter(|&i| canvas[i] == 0).collect();
        assert_eq!(inked, [5]);
    }

    #[test]
    fn keys_compare_text_and_rect() {
        let a = TextKey::new("freeware", Rect::new(0, 450, 100, 480));
        assert_eq!(a, TextKey::new("freeware", Rect::new(0, 450, 100, 480)));
        assert_ne!(a, TextKey::new("freeware", Rect::new(0, 450, 101, 480)));
        assert_ne!(a, TextKey::new("(won)", Rect::new(0, 450, 100, 480)));
    }

    #[test]
    fn lucida_grande_is_installed() {
        let font = system_font(1);
        // SAFETY: `font` is a live CTFont.
        assert_eq!(unsafe { font.post_script_name() }.to_string(), FONT_NAME);
    }

    #[test]
    fn text_is_inked_from_the_top_left_and_wraps() {
        let font = system_font(1);
        let one_line = rasterize(&font, "freeware", 100, 30);
        let rows = |m: &TextMask| -> Vec<usize> {
            (0..m.height)
                .filter(|&y| {
                    m.alpha[y * m.width..(y + 1) * m.width]
                        .iter()
                        .any(|&a| a > 0)
                })
                .collect()
        };
        let inked = rows(&one_line);
        assert!(!inked.is_empty() && inked[0] < 6, "{inked:?}");
        assert!(*inked.last().unwrap() < 20, "{inked:?}");
        let first_column = (0..one_line.width)
            .find(|&x| (0..one_line.height).any(|y| one_line.alpha[y * one_line.width + x] > 0));
        assert!(first_column.is_some_and(|x| x < 4), "{first_column:?}");

        let wrapped = rasterize(
            &font,
            "Click on a level or press spacebar for next.",
            170,
            350,
        );
        let inked = rows(&wrapped);
        assert!(*inked.last().unwrap() > 20, "two or more lines: {inked:?}");
    }

    #[test]
    fn the_cache_rasterizes_once_per_box_and_scale() {
        let mut cache = TypesetCache::default();
        let rect = Rect::new(0, 450, 100, 480);
        let first = cache.mask("freeware", rect, 2).clone();
        assert_eq!((first.width, first.height), (200, 60));
        assert_eq!(cache.mask("freeware", rect, 2), &first);
        cache.mask("(won)", Rect::new(350, 385, 400, 420), 2);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.mask("freeware", rect, 1).width, 100);
        assert_eq!(cache.len(), 1);
    }
}
