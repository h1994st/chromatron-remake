//! The modern look's captions on macOS (spec §10, ruling P3): halation's `Caption`s laid out as
//! the page lays its spans (`web/main.js` `renderCaptions`, `widenCompactName`), set by Core
//! Text into alpha masks (a cache that keeps only what the last paint used), and inked into the
//! canvas as coloured source-over in the palette's text tones. Classic's text stays in
//! `typeset.rs`.

use std::ffi::c_void;
use std::ptr;

use halation::captions::MAX_CAPTIONS;
use halation::tableau::BADGE_W;
use halation::{Caption, CaptionStyle, DeviceRect, MAX_DAMAGE, Palette, Rgb};
use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType,
    CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo, CGPath};
use objc2_core_text::{
    CTFont, CTFontUIFontType, CTFramesetter, CTLine, CTLineTruncationType, kCTFontAttributeName,
    kCTFontTraitsAttribute, kCTFontWeightTrait,
};

use crate::typeset::TextMask;

/// The page's `BADGE_NAME_GAP` and `BADGE_TEXT_GAP` (pt; `web/main.js`, its twin).
pub const BADGE_NAME_GAP_PT: f32 = 4.0;
pub const BADGE_TEXT_GAP_PT: f32 = 8.0;

/// At most this many spans on one row: the damage of two paints (ruling P4).
pub const MAX_SPANS: usize = 2 * MAX_DAMAGE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Centre,
}

/// One caption as the shell sets it: credits sharing a rect are one line.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionLine {
    pub text: String,
    pub rect: DeviceRect,
    pub style: CaptionStyle,
    pub alpha: f32,
    pub align: Align,
}

/// The lines of one paint, in slots that live between paints: `lay_out` clears and refills each
/// slot's `String`, so once the buffers are warm a layout allocates nothing.
#[derive(Debug)]
pub struct CaptionLines {
    slots: Vec<CaptionLine>,
    len: usize,
}

impl Default for CaptionLines {
    fn default() -> Self {
        CaptionLines::new()
    }
}

impl CaptionLines {
    pub fn new() -> Self {
        CaptionLines {
            slots: Vec::with_capacity(MAX_CAPTIONS),
            len: 0,
        }
    }

    /// The next slot, emptied of text but keeping its capacity.
    fn next(&mut self) -> &mut CaptionLine {
        if self.len == self.slots.len() {
            self.slots.push(CaptionLine {
                text: String::new(),
                rect: DeviceRect::default(),
                style: CaptionStyle::default(),
                alpha: 1.0,
                align: Align::Left,
            });
        }
        let slot = &mut self.slots[self.len];
        self.len += 1;
        slot.text.clear();
        slot
    }

    /// The capacity held by the slots and their texts, to tell growth from reuse.
    pub fn capacity(&self) -> usize {
        self.slots.capacity() + self.slots.iter().map(|l| l.text.capacity()).sum::<usize>()
    }
}

impl std::ops::Deref for CaptionLines {
    type Target = [CaptionLine];

    fn deref(&self) -> &[CaptionLine] {
        &self.slots[..self.len]
    }
}

impl std::ops::DerefMut for CaptionLines {
    fn deref_mut(&mut self) -> &mut [CaptionLine] {
        &mut self.slots[..self.len]
    }
}

fn is_empty(r: DeviceRect) -> bool {
    r.left >= r.right || r.top >= r.bottom
}

fn align_of(style: CaptionStyle) -> Align {
    match style {
        CaptionStyle::TitleCompact | CaptionStyle::Badge => Align::Right,
        _ => Align::Left,
    }
}

/// `captions` (one paint's, as shown) as lines into `out`: hidden ones skipped; consecutive
/// credits sharing a rect joined with " · " (ruling 6); a compact card's credits (an empty rect)
/// dropped (ruling P2); a credit spanning the canvas (`canvas_width` px) centred; a compact name
/// widened to the badge's text (`measure` gives a text's width in px at its style).
pub fn lay_out(
    captions: &[Caption<'static>],
    canvas_width: i32,
    px_per_pt: f32,
    measure: impl Fn(&str, CaptionStyle) -> f32,
    out: &mut CaptionLines,
) {
    out.len = 0;
    for c in captions {
        match c.style {
            CaptionStyle::Hidden => {}
            CaptionStyle::Credit if is_empty(c.rect) => {}
            CaptionStyle::Credit => {
                let joined = out
                    .iter_mut()
                    .rev()
                    .find(|l| l.style == CaptionStyle::Credit)
                    .filter(|l| l.rect.left == c.rect.left && l.rect.top == c.rect.top);
                match joined {
                    Some(last) => {
                        last.text.push_str(" · ");
                        last.text.push_str(c.text);
                    }
                    None => {
                        let slot = out.next();
                        slot.text.push_str(c.text);
                        slot.rect = c.rect;
                        slot.style = c.style;
                        slot.alpha = c.alpha;
                        slot.align = Align::Left;
                    }
                }
            }
            style => {
                let slot = out.next();
                slot.text.push_str(c.text);
                slot.rect = c.rect;
                slot.style = style;
                slot.alpha = c.alpha;
                slot.align = align_of(style);
            }
        }
    }
    for l in out.iter_mut().filter(|l| l.style == CaptionStyle::Credit) {
        if (l.rect.left - (canvas_width - l.rect.right)).abs() <= 2 {
            l.align = Align::Centre;
        }
    }
    widen_compact_name(out, px_per_pt, &measure);
}

/// `widenCompactName`: the compact name (halation's rect ends at the badge slot, which Rust
/// can't size to its text) grows right to 8 pt left of the badge's text, or across the empty
/// badge slot, never past the slot.
fn widen_compact_name(
    lines: &mut [CaptionLine],
    px_per_pt: f32,
    measure: &impl Fn(&str, CaptionStyle) -> f32,
) {
    let Some(i) = lines
        .iter()
        .position(|l| l.style == CaptionStyle::TitleCompact)
    else {
        return;
    };
    let right = lines[i].rect.right;
    let px = |pt: f32| (pt * px_per_pt).round() as i32;
    let slot_right = right + px(BADGE_NAME_GAP_PT + BADGE_W);
    let edge = match lines.iter().find(|l| l.style == CaptionStyle::Badge) {
        Some(badge) => {
            badge.rect.right
                - measure(&badge.text, CaptionStyle::Badge).ceil() as i32
                - px(BADGE_TEXT_GAP_PT)
        }
        None => slot_right,
    };
    if edge > right {
        lines[i].rect.right = edge.min(slot_right);
    }
}

struct Entry<M> {
    text: String,
    style: CaptionStyle,
    align: Align,
    width: usize,
    height: usize,
    used: bool,
    mask: M,
}

/// Set captions by (text, style, width, height) at one scale (spec §10). `end` drops what the
/// frame since `begin` didn't use, so it holds one paint's captions (≤ 8): a live resize can't
/// grow it. A hit allocates nothing.
pub struct CaptionCache<M> {
    entries: Vec<Entry<M>>,
}

impl<M> Default for CaptionCache<M> {
    fn default() -> Self {
        CaptionCache::new()
    }
}

impl<M> CaptionCache<M> {
    pub fn new() -> Self {
        CaptionCache {
            entries: Vec::with_capacity(2 * MAX_CAPTIONS),
        }
    }

    pub fn begin(&mut self) {
        for e in &mut self.entries {
            e.used = false;
        }
    }

    pub fn get_or_make(
        &mut self,
        text: &str,
        style: CaptionStyle,
        align: Align,
        width: usize,
        height: usize,
        make: impl FnOnce() -> M,
    ) -> &M {
        let found = self.entries.iter().position(|e| {
            e.style == style
                && e.align == align
                && e.width == width
                && e.height == height
                && e.text == text
        });
        let i = match found {
            Some(i) => i,
            None => {
                self.entries.push(Entry {
                    text: text.to_owned(),
                    style,
                    align,
                    width,
                    height,
                    used: false,
                    mask: make(),
                });
                self.entries.len() - 1
            }
        };
        self.entries[i].used = true;
        &self.entries[i].mask
    }

    pub fn end(&mut self) {
        self.entries.retain(|e| e.used);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The parts of `[left, right)` on row `y` that `rects` cover, merged (overlapping or touching
/// spans become one), left to right, into `out`; their count (ruling P3: each pixel once).
pub fn covered_spans(
    rects: &[DeviceRect],
    y: i32,
    left: i32,
    right: i32,
    out: &mut [(i32, i32); MAX_SPANS],
) -> usize {
    debug_assert!(rects.len() <= MAX_SPANS, "more rects than spans");
    let mut n = 0;
    for r in rects {
        let (a, b) = (r.left.max(left), r.right.min(right));
        if r.top <= y && y < r.bottom && a < b && n < MAX_SPANS {
            // Insertion by start keeps them sorted.
            let mut k = n;
            while k > 0 && out[k - 1].0 > a {
                out[k] = out[k - 1];
                k -= 1;
            }
            out[k] = (a, b);
            n += 1;
        }
    }
    let mut m = 0;
    for k in 0..n {
        if m > 0 && out[k].0 <= out[m - 1].1 {
            out[m - 1].1 = out[m - 1].1.max(out[k].1);
        } else {
            out[m] = out[k];
            m += 1;
        }
    }
    m
}

/// `pixel` (`0x00RRGGBB`) with `colour` over it at `coverage` (0–255) × `alpha` (0–1): source-over
/// in sRGB code values, as the page's text is composited.
pub fn ink(pixel: u32, colour: Rgb, coverage: u8, alpha: f32) -> u32 {
    let a8 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    let a = (u32::from(coverage) * a8 + 127) / 255;
    let mix = |shift: u32, c: u8| {
        let p = (pixel >> shift) & 0xff;
        ((p * (255 - a) + u32::from(c) * a + 127) / 255) << shift
    };
    mix(16, colour.0) | mix(8, colour.1) | mix(0, colour.2)
}

/// Where a stamp may ink: the whole line, or only inside these damage rects.
#[derive(Clone, Copy, Debug)]
pub enum Clip<'r> {
    Whole,
    Within(&'r [DeviceRect]),
}

/// Inks `mask` (the line's set text, its rect's size) at the line's rect into `canvas`
/// (`width × height`, `0x00RRGGBB`), clipped to the canvas and `clip`, each pixel once.
pub fn stamp(
    mask: &TextMask,
    line: &CaptionLine,
    colour: Rgb,
    canvas: &mut [u32],
    width: usize,
    height: usize,
    clip: Clip<'_>,
) {
    let (left, top) = (line.rect.left, line.rect.top);
    let x0 = left.max(0);
    let x1 = (left + mask.width as i32).min(width as i32);
    if x0 >= x1 {
        return;
    }
    let mut spans = [(0, 0); MAX_SPANS];
    for row in 0..mask.height {
        let y = top + row as i32;
        if y < 0 || y as usize >= height {
            continue;
        }
        let n = match clip {
            Clip::Whole => {
                spans[0] = (x0, x1);
                1
            }
            Clip::Within(rects) => covered_spans(rects, y, x0, x1, &mut spans),
        };
        let src_row = row * mask.width;
        let dst_row = y as usize * width;
        for &(a, b) in &spans[..n] {
            let src = &mask.alpha[src_row + (a - left) as usize..src_row + (b - left) as usize];
            let dst = &mut canvas[dst_row + a as usize..dst_row + b as usize];
            for (p, &c) in dst.iter_mut().zip(src) {
                if c != 0 {
                    *p = ink(*p, colour, c, line.alpha);
                }
            }
        }
    }
}

/// `kCTFontWeightTrait` values (AppKit's `NSFontWeight…`) for the page's weights 400, 500, 600.
pub const REGULAR: f64 = 0.0;
pub const MEDIUM: f64 = 0.23;
pub const SEMIBOLD: f64 = 0.3;

/// Body's sizes (pt) and line heights (× size), largest first (spec §10, ruling 22; the page's
/// `BODY_SIZES`).
pub const BODY_STEPS: [(f64, f64); 3] = [(15.0, 1.4), (13.0, 1.4), (11.0, 1.3)];

/// How deep a body frame is laid out (pt): far below any box, so every line is counted.
const BODY_DEPTH: f64 = 1.0e6;

/// How a one-line style shows text too wide for its rect (the page's `text-overflow`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Cut to fit with "…" at the end.
    Ellipsis,
    /// Set whole from the start edge and cut at the rect's edge.
    Clip,
}

/// The page's `.cap-*` rules: the title, the compact name and the credits ellipsise; the badge
/// and the pill clip.
fn overflow_of(style: CaptionStyle) -> Overflow {
    match style {
        CaptionStyle::Title | CaptionStyle::TitleCompact | CaptionStyle::Credit => {
            Overflow::Ellipsis
        }
        _ => Overflow::Clip,
    }
}

/// The palette text tone `style` is set in (spec §10; the page's `--ink`, `--body`, `--muted`).
fn colour(palette: &Palette, style: CaptionStyle) -> Rgb {
    match style {
        CaptionStyle::Body => palette.text.body,
        CaptionStyle::Badge | CaptionStyle::Credit => palette.text.muted,
        _ => palette.text.primary,
    }
}

/// `text` with its white space collapsed as the page's CSS collapses it, after `renderCaptions`
/// turns `\r\n` and `\r` into `\n`: runs of spaces, tabs and (unless `breaks`) line breaks become
/// one space, and none is left at a line's ends. With `breaks` (`pre-line`, Body) each break
/// stays a line break; trailing ones go, as they make no line box.
fn collapse_white_space(text: &str, breaks: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = if c == '\r' {
            chars.next_if_eq(&'\n');
            '\n'
        } else {
            c
        };
        match c {
            '\n' if breaks => {
                space = false;
                out.push('\n');
            }
            ' ' | '\t' | '\n' => space = true,
            c => {
                if space && !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                space = false;
                out.push(c);
            }
        }
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// The system UI font (the page's `system-ui`) at `points` and `weight` (`kCTFontWeightTrait`).
///
/// The captions are laid out in points and drawn through a context scaled to the display, as the
/// page lays them out in CSS px: the system font picks its optical size and tracking from its
/// size, so a font made at the pixel size would set @2 text about 10% narrower than the page's.
pub fn ui_font(points: f64, weight: f64) -> CFRetained<CTFont> {
    // SAFETY: a valid UI font type; no language.
    let base = unsafe { CTFont::new_ui_font_for_language(CTFontUIFontType::System, points, None) }
        .expect("Core Text has a system UI font");
    if weight == REGULAR {
        return base;
    }
    // SAFETY: `base` is a live font; the keys are immutable CFString constants.
    let (descriptor, weight_key, traits_key) = unsafe {
        (
            base.font_descriptor(),
            kCTFontWeightTrait,
            kCTFontTraitsAttribute,
        )
    };
    let number = CFNumber::new_f64(weight);
    let traits = CFDictionary::<CFString, CFType>::from_slices(&[weight_key], &[number.as_ref()]);
    let attributes = CFDictionary::<CFString, CFType>::from_slices(
        &[traits_key],
        &[traits.as_opaque().as_ref()],
    );
    // SAFETY: the attributes map the traits key to a traits dictionary, as Core Text requires; a
    // null matrix means identity.
    unsafe {
        let weighted = descriptor.copy_with_attributes(attributes.as_opaque());
        CTFont::with_font_descriptor(&weighted, points, ptr::null())
    }
}

/// Body's three fonts (`BODY_STEPS`).
pub fn body_fonts() -> [CFRetained<CTFont>; 3] {
    BODY_STEPS.map(|(points, _)| ui_font(points, REGULAR))
}

/// An alpha-only bitmap (`width × height` px, as `typeset::rasterize` makes) with the Core
/// Graphics context that draws into it in points. The context writes through a raw pointer into
/// `alpha`'s buffer, so the two are owned together: the context is never handed out, `alpha` is
/// never reallocated, and the context is released before the buffer (`into_mask`, and the field
/// order on drop).
struct AlphaBitmap {
    // Declared before `alpha`, so dropped before it.
    context: CFRetained<CGContext>,
    alpha: Vec<u8>,
    width: usize,
    height: usize,
}

impl AlphaBitmap {
    /// A clear bitmap; `width` and `height` are not 0.
    fn new(width: usize, height: usize, px_per_pt: f64) -> AlphaBitmap {
        let mut alpha = vec![0u8; width * height];
        // SAFETY: `alpha` is `width × height` bytes; its heap buffer does not move when the Vec
        // moves into `self`, is never resized, and outlives the context (see the type's doc). An
        // alpha-only context with 8 bits per component and `width` bytes per row takes no
        // colour space.
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
        CGContext::scale_ctm(Some(&context), px_per_pt, px_per_pt);
        AlphaBitmap {
            context,
            alpha,
            width,
            height,
        }
    }

    /// Draws `line` with its origin `x` pt right of the left edge and its baseline `y` pt above
    /// the foot.
    fn draw(&self, line: &CTLine, x: f64, y: f64) {
        CGContext::set_text_position(Some(&self.context), x, y);
        // SAFETY: a live line and a live context over a live buffer.
        unsafe { line.draw(&self.context) };
    }

    /// The coverage drawn, the context released first.
    fn into_mask(self) -> TextMask {
        let AlphaBitmap {
            context,
            alpha,
            width,
            height,
        } = self;
        drop(context);
        TextMask {
            width,
            height,
            alpha,
        }
    }
}

fn empty_mask(width: usize, height: usize) -> TextMask {
    TextMask {
        width,
        height,
        alpha: vec![0u8; width * height],
    }
}

/// `text` in `font`.
fn attributed(font: &CTFont, text: &str) -> CFRetained<CFAttributedString> {
    // SAFETY: an immutable CFString constant.
    let key = unsafe { kCTFontAttributeName };
    let attributes = CFDictionary::<CFString, CFType>::from_slices(&[key], &[font.as_ref()]);
    // SAFETY: valid CF objects; the dictionary maps Core Text's attribute keys to their types.
    unsafe {
        CFAttributedString::new(
            None,
            Some(&CFString::from_str(text)),
            Some(attributes.as_opaque()),
        )
    }
    .expect("Core Foundation makes an attributed string")
}

/// `text`'s width (pt) on one line in `font`, its white space collapsed as the page's.
pub fn line_width(font: &CTFont, text: &str) -> f64 {
    let text = collapse_white_space(text, false);
    // SAFETY: a valid attributed string; null out-pointers are allowed.
    unsafe {
        CTLine::with_attributed_string(&attributed(font, &text)).typographic_bounds(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
}

/// One line of `text` (`nowrap`) in `font` (pt) at `px_per_pt` in a `width × height` px box:
/// aligned, too wide shown as `overflow` says, and the font's ascent and descent centred in the
/// box, where a CSS line box as tall as the box puts them (each one-line style's line height is
/// its rect's height), on a whole pixel.
pub fn rasterize_line(
    font: &CTFont,
    px_per_pt: f32,
    text: &str,
    align: Align,
    overflow: Overflow,
    width: usize,
    height: usize,
) -> TextMask {
    if width == 0 || height == 0 {
        return empty_mask(width, height);
    }
    let text = collapse_white_space(text, false);
    let scale = f64::from(px_per_pt);
    let bitmap = AlphaBitmap::new(width, height, scale);
    let room_pt = width as f64 / scale;
    // SAFETY: valid attributed strings and a live font throughout.
    unsafe {
        let mut line = CTLine::with_attributed_string(&attributed(font, &text));
        let full = line.typographic_bounds(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        if overflow == Overflow::Ellipsis && full > room_pt {
            let token = CTLine::with_attributed_string(&attributed(font, "…"));
            if let Some(cut) = line.truncated_line(room_pt, CTLineTruncationType::End, Some(&token))
            {
                line = cut;
            }
        }
        // CSS sets a line too wide for its box from the start edge, whatever the alignment,
        // and only then cuts it to an ellipsis.
        let spare = (room_pt - full).max(0.0);
        let x = match align {
            Align::Left => 0.0,
            Align::Right => spare,
            Align::Centre => spare / 2.0,
        };
        // CG's origin is the bottom left: the baseline lies the half-leading plus the descent
        // above the box's foot.
        let (ascent, descent) = (font.ascent() * scale, font.descent() * scale);
        let y = ((height as f64 - (ascent + descent)) / 2.0 + descent).round();
        bitmap.draw(&line, x, y / scale);
    }
    bitmap.into_mask()
}

/// CSS px rounded as Blink snaps a box's size to whole pixels (`LayoutUnit::Round`: half up).
fn snap(css_px: f64) -> f64 {
    (css_px + 0.5).floor()
}

/// Body text (spec §10, ruling 22; `pre-line`): line breaks kept, words wrapped at the box's
/// width; set at 15 pt, else 13, else 11 (line height 1.4, 1.4, 1.3), the first whose lines fit
/// the box as the page judges it (`scrollHeight <= clientHeight + 1`, both snapped to whole CSS
/// px as Blink snaps them, the box's top taken to lie on a whole CSS px); at 11 pt the box's
/// foot cuts it. Line n's box starts n line heights below the box's top, the font's ascent and
/// descent centred in it as in CSS, its baseline on a whole pixel. `fonts` are `body_fonts`, set
/// at `px_per_pt` into a `width × height` px box. Returns the step used and the mask.
pub fn body_fit(
    fonts: &[CFRetained<CTFont>; 3],
    px_per_pt: f32,
    text: &str,
    width: usize,
    height: usize,
) -> (usize, TextMask) {
    if width == 0 || height == 0 {
        return (0, empty_mask(width, height));
    }
    let text = collapse_white_space(text, true);
    let scale = f64::from(px_per_pt);
    // The page's `clientHeight + 1`: the content may overrun the box by 1 CSS px.
    let room = snap(height as f64 / scale) + 1.0;
    let last = BODY_STEPS.len() - 1;
    for (step, (font, &(points, lh))) in fonts.iter().zip(&BODY_STEPS).enumerate() {
        let line_pt = points * lh;
        // SAFETY: `font` is live; valid attributed strings, paths and frames throughout.
        let drawn = unsafe {
            // Broken into lines in a column deep enough to hold them all, so all are counted.
            let column = CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(width as f64 / scale, BODY_DEPTH),
            );
            let path = CGPath::with_rect(column, ptr::null());
            let frame = CTFramesetter::with_attributed_string(&attributed(font, &text)).frame(
                CFRange::new(0, 0),
                &path,
                None,
            );
            let lines = frame.lines();
            // The page's `scrollHeight`: the line boxes' height, snapped.
            let fits = snap(lines.len() as f64 * line_pt) <= room;
            if fits || step == last {
                let bitmap = AlphaBitmap::new(width, height, scale);
                let (ascent, descent) = (font.ascent(), font.descent());
                let first = (line_pt - (ascent + descent)) / 2.0 + ascent;
                for i in 0..lines.len() {
                    let baseline = ((i as f64 * line_pt + first) * scale).round();
                    if baseline - ascent * scale >= height as f64 {
                        break;
                    }
                    // The frame's lines are CTLines; the array lives, unchanged, with `frame`.
                    let line = &*lines.value_at_index(i as CFIndex).cast::<CTLine>();
                    // CG's origin is the bottom left.
                    bitmap.draw(line, 0.0, (height as f64 - baseline) / scale);
                }
                Some(bitmap.into_mask())
            } else {
                None
            }
        };
        if let Some(mask) = drawn {
            return (step, mask);
        }
    }
    unreachable!("the last step always draws")
}

/// The modern captions' fonts (the page's `.cap-*` fonts), in points.
struct Fonts {
    title: CFRetained<CTFont>,
    title_compact: CFRetained<CTFont>,
    medium: CFRetained<CTFont>,
    credit: CFRetained<CTFont>,
    body: [CFRetained<CTFont>; 3],
}

impl Fonts {
    fn new() -> Fonts {
        Fonts {
            title: ui_font(17.0, SEMIBOLD),
            title_compact: ui_font(15.0, SEMIBOLD),
            medium: ui_font(13.0, MEDIUM),
            credit: ui_font(12.0, REGULAR),
            body: body_fonts(),
        }
    }

    /// The one-line font for `style` (`None`: Body, set by `body_fit`, and Hidden).
    fn line(&self, style: CaptionStyle) -> Option<&CFRetained<CTFont>> {
        match style {
            CaptionStyle::Title => Some(&self.title),
            CaptionStyle::TitleCompact => Some(&self.title_compact),
            CaptionStyle::Badge | CaptionStyle::Pill => Some(&self.medium),
            CaptionStyle::Credit => Some(&self.credit),
            CaptionStyle::Body | CaptionStyle::Hidden => None,
        }
    }
}

/// Sets and inks the modern captions at one scale (spec §10 "macOS").
pub struct CaptionTypesetter {
    px_per_pt: f32,
    fonts: Fonts,
    cache: CaptionCache<TextMask>,
    sets: usize,
}

impl CaptionTypesetter {
    pub fn new(px_per_pt: f32) -> CaptionTypesetter {
        CaptionTypesetter {
            px_per_pt,
            fonts: Fonts::new(),
            cache: CaptionCache::new(),
            sets: 0,
        }
    }

    /// A new scale (another display): nothing cached.
    pub fn set_scale(&mut self, px_per_pt: f32) {
        if px_per_pt != self.px_per_pt {
            self.px_per_pt = px_per_pt;
            self.cache.clear();
        }
    }

    /// `text`'s width in px as `style` sets it on one line (Body: at 15 pt).
    pub fn measure(&self, text: &str, style: CaptionStyle) -> f32 {
        let font = self.fonts.line(style).unwrap_or(&self.fonts.body[0]);
        (line_width(font, text) * f64::from(self.px_per_pt)) as f32
    }

    /// Inks every line (setting the ones not cached), each within `clip`; the cache then holds
    /// these lines only. A line already set calls no Core Text and allocates nothing.
    pub fn ink(
        &mut self,
        lines: &[CaptionLine],
        palette: &Palette,
        canvas: &mut [u32],
        width: usize,
        height: usize,
        clip: Clip<'_>,
    ) {
        self.cache.begin();
        for line in lines {
            if line.style == CaptionStyle::Hidden {
                continue;
            }
            let w = (line.rect.right - line.rect.left).max(0) as usize;
            let h = (line.rect.bottom - line.rect.top).max(0) as usize;
            if w == 0 || h == 0 {
                continue;
            }
            let (fonts, px_per_pt, sets) = (&self.fonts, self.px_per_pt, &mut self.sets);
            let make = || {
                *sets += 1;
                match fonts.line(line.style) {
                    Some(font) => {
                        let (align, overflow) = (line.align, overflow_of(line.style));
                        rasterize_line(font, px_per_pt, &line.text, align, overflow, w, h)
                    }
                    None => body_fit(&fonts.body, px_per_pt, &line.text, w, h).1,
                }
            };
            let mask = self
                .cache
                .get_or_make(&line.text, line.style, line.align, w, h, make);
            stamp(
                mask,
                line,
                colour(palette, line.style),
                canvas,
                width,
                height,
                clip,
            );
        }
        self.cache.end();
    }

    /// How many set captions are cached.
    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    /// How many captions Core Text has set since the typesetter was made.
    pub fn sets(&self) -> usize {
        self.sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halation::CaptionRole;
    use lumen::TextRole;

    fn cap(
        role: TextRole,
        text: &'static str,
        rect: DeviceRect,
        style: CaptionStyle,
    ) -> Caption<'static> {
        Caption {
            role: CaptionRole::Text(role),
            text,
            rect,
            style,
            alpha: 1.0,
        }
    }

    fn laid(
        captions: &[Caption<'static>],
        measure: impl Fn(&str, CaptionStyle) -> f32,
    ) -> CaptionLines {
        let mut out = CaptionLines::new();
        lay_out(captions, 1000, 1.0, measure, &mut out);
        out
    }

    const NONE: fn(&str, CaptionStyle) -> f32 = |_, _| 0.0;

    #[test]
    fn hidden_captions_are_not_laid_out() {
        let rect = DeviceRect::new(0, 0, 100, 20);
        let lines = laid(
            &[cap(TextRole::Win, "YOU WIN!", rect, CaptionStyle::Hidden)],
            NONE,
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn credits_sharing_a_rect_join_with_a_dot() {
        let a = DeviceRect::new(600, 700, 980, 718);
        let b = DeviceRect::new(10, 750, 400, 768);
        let lines = laid(
            &[
                cap(TextRole::Freeware, "Freeware", a, CaptionStyle::Credit),
                cap(TextRole::ThankYou, "Thank you", a, CaptionStyle::Credit),
                cap(TextRole::SiteUrl, "site", b, CaptionStyle::Credit),
            ],
            NONE,
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Freeware · Thank you");
        assert_eq!(lines[1].text, "site");
    }

    #[test]
    fn a_compact_cards_credits_are_not_shown() {
        let empty = DeviceRect::default();
        let lines = laid(
            &[cap(
                TextRole::Freeware,
                "Freeware",
                empty,
                CaptionStyle::Credit,
            )],
            NONE,
        );
        assert!(lines.is_empty(), "ruling P2: no Help sheet on macOS");
    }

    #[test]
    fn a_credit_across_the_canvas_is_centred() {
        let centred = laid(
            &[cap(
                TextRole::SiteUrl,
                "site",
                DeviceRect::new(16, 700, 984, 718),
                CaptionStyle::Credit,
            )],
            NONE,
        );
        assert_eq!(centred[0].align, Align::Centre);
        let left = laid(
            &[cap(
                TextRole::SiteUrl,
                "site",
                DeviceRect::new(600, 700, 980, 718),
                CaptionStyle::Credit,
            )],
            NONE,
        );
        assert_eq!(left[0].align, Align::Left);
    }

    #[test]
    fn styles_align_as_the_page_does() {
        let r = DeviceRect::new(0, 0, 100, 28);
        let lines = laid(
            &[
                cap(TextRole::LevelName, "Name", r, CaptionStyle::Title),
                cap(TextRole::Won, "(won)", r, CaptionStyle::Badge),
                cap(TextRole::Hint, "Hint", r, CaptionStyle::Body),
            ],
            NONE,
        );
        let aligns: Vec<Align> = lines.iter().map(|l| l.align).collect();
        assert_eq!(aligns, [Align::Left, Align::Right, Align::Left]);
    }

    #[test]
    fn compact_titles_align_right_and_pills_left() {
        let r = DeviceRect::new(0, 0, 100, 28);
        let lines = laid(
            &[
                cap(TextRole::LevelName, "Name", r, CaptionStyle::TitleCompact),
                cap(TextRole::Hint, "Levels", r, CaptionStyle::Pill),
            ],
            NONE,
        );
        assert_eq!(lines[0].align, Align::Right);
        assert_eq!(lines[1].align, Align::Left);
    }

    #[test]
    fn a_compact_name_widens_at_two_pixels_per_point() {
        // Slot: 2 * (4 + 44) = 96 px; the badge text is 30 px and the gap 8 pt = 16 px.
        let name = cap(
            TextRole::LevelName,
            "Long level name",
            DeviceRect::new(300, 10, 500, 38),
            CaptionStyle::TitleCompact,
        );
        let badge = cap(
            TextRole::Won,
            "(won)",
            DeviceRect::new(508, 10, 596, 38),
            CaptionStyle::Badge,
        );
        let mut out = CaptionLines::new();
        lay_out(&[name, badge], 1000, 2.0, |_, _| 30.0, &mut out);
        assert_eq!(out[0].rect.right, 596 - 30 - 16);
        lay_out(&[name], 1000, 2.0, NONE, &mut out);
        assert_eq!(out[0].rect.right, 500 + 96);
    }

    #[test]
    fn laying_out_again_reuses_the_buffers() {
        let a = DeviceRect::new(600, 700, 980, 718);
        let caps = [
            cap(TextRole::Freeware, "Freeware", a, CaptionStyle::Credit),
            cap(TextRole::ThankYou, "Thank you", a, CaptionStyle::Credit),
            cap(TextRole::Hint, "Hint", a, CaptionStyle::Body),
        ];
        let mut out = CaptionLines::new();
        lay_out(&caps, 1000, 1.0, NONE, &mut out);
        let warm = out.capacity();
        for _ in 0..3 {
            lay_out(&caps, 1000, 1.0, NONE, &mut out);
            assert_eq!(out.capacity(), warm);
        }
        assert_eq!(out[0].text, "Freeware · Thank you");
        // A shorter layout after a longer one leaves no stale text.
        lay_out(&caps[2..], 1000, 1.0, NONE, &mut out);
        assert_eq!((out.len(), &*out[0].text), (1, "Hint"));
    }

    #[test]
    fn a_compact_name_widens_to_the_badges_text() {
        // Name ends at 500; the badge slot is 504..548 (4 + 44 pt at 1 px/pt); "(won)" is 30 px.
        let name = cap(
            TextRole::LevelName,
            "Long level name",
            DeviceRect::new(300, 10, 500, 38),
            CaptionStyle::TitleCompact,
        );
        let badge = cap(
            TextRole::Won,
            "(won)",
            DeviceRect::new(504, 10, 548, 38),
            CaptionStyle::Badge,
        );
        let lines = laid(&[name, badge], |_, _| 30.0);
        assert_eq!(lines[0].rect.right, 548 - 30 - 8);
        // No badge: across the empty slot.
        let lines = laid(&[name], NONE);
        assert_eq!(lines[0].rect.right, 500 + 4 + 44);
        // A badge text too wide to leave room: unchanged.
        let lines = laid(&[name, badge], |_, _| 44.0);
        assert_eq!(lines[0].rect.right, 500);
    }

    #[test]
    fn the_cache_makes_once_and_keeps_only_what_a_frame_used() {
        let mut cache = CaptionCache::new();
        let made = std::cell::Cell::new(0);
        let make = |v: u8| {
            made.set(made.get() + 1);
            v
        };
        cache.begin();
        let l = Align::Left;
        assert_eq!(
            *cache.get_or_make("a", CaptionStyle::Body, l, 10, 10, || make(1)),
            1
        );
        assert_eq!(
            *cache.get_or_make("a", CaptionStyle::Body, l, 10, 10, || make(9)),
            1
        );
        cache.get_or_make("a", CaptionStyle::Body, l, 11, 10, || make(2));
        // The same text, style and size aligned differently is another mask.
        cache.get_or_make("a", CaptionStyle::Body, Align::Centre, 11, 10, || make(3));
        cache.end();
        assert_eq!((made.get(), cache.len()), (3, 3));
        // Many frames of new captions never hold more than one frame's worth.
        for frame in 0..100 {
            cache.begin();
            for k in 0..MAX_CAPTIONS {
                cache.get_or_make("x", CaptionStyle::Credit, l, frame, k, || make(0));
            }
            cache.end();
            assert_eq!(cache.len(), MAX_CAPTIONS);
        }
    }

    #[test]
    fn overlapping_rects_ink_each_pixel_once() {
        let mut spans = [(0, 0); MAX_SPANS];
        let rects = [
            DeviceRect::new(0, 0, 50, 10),
            DeviceRect::new(30, 0, 80, 10),
            DeviceRect::new(100, 0, 120, 10),
            DeviceRect::new(119, 5, 140, 10),
            DeviceRect::new(0, 20, 200, 30),
        ];
        let n = covered_spans(&rects, 2, 10, 130, &mut spans);
        assert_eq!(&spans[..n], [(10, 80), (100, 120)]);
        let n = covered_spans(&rects, 7, 10, 130, &mut spans);
        assert_eq!(&spans[..n], [(10, 80), (100, 130)], "touching spans merge");
        assert_eq!(covered_spans(&rects, 15, 0, 200, &mut spans), 0);
    }

    #[test]
    fn ink_is_source_over_in_the_tone() {
        let page = 0x0010_2030;
        let tone = Rgb(0xe8, 0xed, 0xff);
        assert_eq!(ink(page, tone, 0, 1.0), page);
        assert_eq!(ink(page, tone, 255, 1.0), 0x00e8_edff);
        assert_eq!(ink(page, tone, 255, 0.0), page);
        let half = ink(0, Rgb(200, 100, 0), 255, 0.5);
        assert_eq!(half, (100 << 16) | (50 << 8));
    }

    #[test]
    fn stamping_within_damage_touches_only_its_spans() {
        let mask = TextMask {
            width: 4,
            height: 2,
            alpha: vec![255; 8],
        };
        let line = CaptionLine {
            text: "x".into(),
            rect: DeviceRect::new(2, 1, 6, 3),
            style: CaptionStyle::Body,
            alpha: 1.0,
            align: Align::Left,
        };
        let page = 0x0010_2030;
        let tone = Rgb(255, 255, 255);
        let once = ink(page, tone, 128, 0.5);
        assert_ne!(ink(once, tone, 128, 0.5), once, "inking twice would show");
        let line = CaptionLine { alpha: 0.5, ..line };
        let mask = TextMask {
            alpha: vec![128; 8],
            ..mask
        };
        let mut canvas = vec![page; 8 * 4];
        let damage = [DeviceRect::new(0, 0, 4, 4), DeviceRect::new(3, 0, 5, 4)];
        stamp(&mask, &line, tone, &mut canvas, 8, 4, Clip::Within(&damage));
        let inked: Vec<usize> = (0..32).filter(|&i| canvas[i] != page).collect();
        assert_eq!(
            inked,
            [10, 11, 12, 18, 19, 20],
            "columns 2–4 of rows 1–2, once each"
        );
        assert!(
            inked.iter().all(|&i| canvas[i] == once),
            "column 3 lies in both rects and is still one application"
        );
        let mut whole = vec![page; 8 * 4];
        stamp(&mask, &line, tone, &mut whole, 8, 4, Clip::Whole);
        assert_eq!(whole.iter().filter(|&&p| p == once).count(), 8);
    }

    #[test]
    fn spans_that_only_touch_merge() {
        let mut spans = [(0, 0); MAX_SPANS];
        let rects = [DeviceRect::new(0, 0, 50, 4), DeviceRect::new(50, 0, 90, 4)];
        let n = covered_spans(&rects, 1, 0, 100, &mut spans);
        assert_eq!(&spans[..n], [(0, 90)]);
        // Out of order, the same.
        let rects = [DeviceRect::new(50, 0, 90, 4), DeviceRect::new(0, 0, 50, 4)];
        let n = covered_spans(&rects, 1, 0, 100, &mut spans);
        assert_eq!(&spans[..n], [(0, 90)]);
    }

    #[test]
    fn stamping_clips_at_the_canvas_edges() {
        let mask = TextMask {
            width: 4,
            height: 3,
            alpha: (1..=12).collect(),
        };
        let line = CaptionLine {
            text: "x".into(),
            rect: DeviceRect::new(-2, 2, 2, 5),
            style: CaptionStyle::Body,
            alpha: 1.0,
            align: Align::Left,
        };
        let tone = Rgb(255, 255, 255);
        // A 6x3 canvas: mask row 0 lands on its last row; the other two rows lie below it.
        let mut canvas = vec![0u32; 6 * 3];
        stamp(&mask, &line, tone, &mut canvas, 6, 3, Clip::Whole);
        let inked: Vec<usize> = (0..18).filter(|&i| canvas[i] != 0).collect();
        // Row 2, mask columns 2 and 3 (x = 0, 1); columns 0 and 1 are off the left edge.
        assert_eq!(inked, [12, 13]);
        assert_eq!(canvas[12], ink(0, tone, 3, 1.0));
        assert_eq!(canvas[13], ink(0, tone, 4, 1.0));
        let mut again = vec![0u32; 6 * 3];
        let damage = [DeviceRect::new(0, 0, 6, 3)];
        stamp(&mask, &line, tone, &mut again, 6, 3, Clip::Within(&damage));
        assert_eq!(again, canvas);
    }

    fn rows(m: &TextMask) -> Vec<usize> {
        (0..m.height)
            .filter(|&y| {
                m.alpha[y * m.width..(y + 1) * m.width]
                    .iter()
                    .any(|&a| a > 0)
            })
            .collect()
    }

    fn columns(m: &TextMask) -> Vec<usize> {
        (0..m.width)
            .filter(|&x| (0..m.height).any(|y| m.alpha[y * m.width + x] > 0))
            .collect()
    }

    #[test]
    fn the_weights_are_the_faces_core_text_picked() {
        // The resolved font's own weight: a trait Core Text ignored would read 0 (Regular).
        let weight = |asked| {
            // SAFETY: a live font; an immutable CFString constant. The traits dictionary maps
            // CFString keys to CF values, as Core Text documents.
            let traits = unsafe { ui_font(17.0, asked).traits() };
            let traits = unsafe { traits.cast_unchecked::<CFString, CFType>() };
            let value = traits
                .get(unsafe { kCTFontWeightTrait })
                .expect("a weight trait");
            let number = value.downcast_ref::<CFNumber>().expect("a number");
            number.as_f64().expect("a float")
        };
        for asked in [REGULAR, MEDIUM, SEMIBOLD] {
            let got = weight(asked);
            assert!((got - asked).abs() < 0.01, "asked {asked}, got {got}");
        }
    }

    #[test]
    fn text_is_laid_out_in_points_as_the_page_does() {
        // The page lays out in CSS px and draws at the display's scale; a font made at the
        // pixel size takes the system font's tighter large-size spacing (the credit then
        // measures 314.6 px at @2 against the page's 349).
        let t = "freeware · silverspaceship.com";
        let two = CaptionTypesetter::new(2.0).measure(t, CaptionStyle::Credit);
        let pixel_sized = line_width(&ui_font(24.0, REGULAR), t) as f32;
        assert!(two > pixel_sized * 1.05, "{two} vs {pixel_sized}");
        assert!(
            (two - 349.0).abs() < 3.5,
            "the page measures it 349 px: {two}"
        );
        let font = ui_font(12.0, REGULAR);
        let width = |px_per_pt: f32| {
            let w = (420.0 * px_per_pt) as usize;
            let h = (18.0 * px_per_pt) as usize;
            let mask = rasterize_line(&font, px_per_pt, t, Align::Left, Overflow::Clip, w, h);
            *columns(&mask).last().unwrap() as f32
        };
        assert!((width(2.0) - 2.0 * width(1.0)).abs() <= 2.0);
    }

    #[test]
    fn a_title_is_one_line_centred_in_its_row() {
        let font = ui_font(17.0, SEMIBOLD);
        let mask = rasterize_line(
            &font,
            1.0,
            "Level name",
            Align::Left,
            Overflow::Ellipsis,
            300,
            40,
        );
        let inked = rows(&mask);
        let middle = (inked[0] + inked.last().unwrap()) / 2;
        assert!((16..=24).contains(&middle), "{inked:?}");
        assert!(columns(&mask)[0] < 4);
    }

    #[test]
    fn a_one_line_baseline_sits_where_the_pages_line_box_puts_it() {
        // CSS: a line box as tall as the row, the font's ascent and descent centred in it.
        for (points, weight, height) in [
            (17.0, SEMIBOLD, 22.0),
            (13.0, MEDIUM, 28.0),
            (12.0, REGULAR, 18.0),
        ] {
            let font = ui_font(points, weight);
            // SAFETY: a live font.
            let (a, d) = unsafe { (font.ascent(), font.descent()) };
            let baseline = 2.0 * ((height - (a + d)) / 2.0 + a);
            let h = 2 * height as usize;
            let mask = rasterize_line(&font, 2.0, "HH", Align::Left, Overflow::Clip, 100, h);
            let foot = *rows(&mask).last().unwrap() as f64 + 1.0;
            assert!(
                (foot - baseline).abs() <= 1.0,
                "{points} pt: {foot} vs {baseline}"
            );
        }
    }

    #[test]
    fn right_aligned_text_ends_at_the_right_edge() {
        let font = ui_font(13.0, MEDIUM);
        let set = |align| rasterize_line(&font, 1.0, "(won)", align, Overflow::Clip, 100, 28);
        let cols = columns(&set(Align::Right));
        assert!(*cols.last().unwrap() >= 92 && cols[0] > 40, "{cols:?}");
        let cols = columns(&set(Align::Centre));
        let middle = (cols[0] + cols.last().unwrap()) / 2;
        assert!((48..=52).contains(&middle), "{cols:?}");
    }

    #[test]
    fn a_long_title_ends_in_an_ellipsis_inside_its_box() {
        let font = ui_font(17.0, SEMIBOLD);
        let words = "Wavelength ".repeat(20);
        let set = |align| rasterize_line(&font, 1.0, &words, align, Overflow::Ellipsis, 200, 40);
        let mask = set(Align::Left);
        let (cols, inked) = (columns(&mask), rows(&mask));
        assert!(cols.len() > 150 && *cols.last().unwrap() < 200, "{cols:?}");
        assert!(inked.last().unwrap() - inked[0] < 25, "one line: {inked:?}");
        // CSS sets a right-aligned name too long for its rect from the start edge, then cuts it.
        assert_eq!(set(Align::Right), mask);
    }

    #[test]
    fn one_line_styles_ellipsise_or_clip_as_the_page_does() {
        use CaptionStyle::*;
        let ellipsised = [Title, TitleCompact, Credit].map(overflow_of);
        assert_eq!(ellipsised, [Overflow::Ellipsis; 3]);
        assert_eq!([Badge, Pill].map(overflow_of), [Overflow::Clip; 2]);
        let font = ui_font(13.0, MEDIUM);
        let words = "Wavelength ".repeat(20);
        let set = |align, overflow| rasterize_line(&font, 1.0, &words, align, overflow, 200, 28);
        let clipped = set(Align::Left, Overflow::Clip);
        assert_ne!(set(Align::Left, Overflow::Ellipsis), clipped);
        assert!(
            *columns(&clipped).last().unwrap() >= 196,
            "clipped mid-word at the edge"
        );
        // CSS starts text too wide for its box at the start edge, whatever its alignment.
        assert_eq!(set(Align::Right, Overflow::Clip), clipped);
    }

    #[test]
    fn white_space_collapses_as_the_pages_css_does() {
        assert_eq!(
            collapse_white_space("  a  b \r\n  c\rd\n", true),
            "a b\nc\nd"
        );
        assert_eq!(collapse_white_space("a\r\n\r\nb", true), "a\n\nb");
        assert_eq!(collapse_white_space("a\r\nb\t c ", false), "a b c");
        let fonts = body_fonts();
        let (_, crlf) = body_fit(&fonts, 1.0, "One\r\nTwo", 300, 200);
        assert_eq!(crlf, body_fit(&fonts, 1.0, "One\rTwo", 300, 200).1);
        assert_eq!(crlf, body_fit(&fonts, 1.0, "One\nTwo", 300, 200).1);
        let font = ui_font(17.0, SEMIBOLD);
        let line =
            |t: &str| rasterize_line(&font, 1.0, t, Align::Left, Overflow::Ellipsis, 300, 30);
        assert_eq!(line("Light\r\nbeam"), line("Light beam"));
    }

    #[test]
    fn body_text_breaks_at_carriage_returns() {
        let fonts = body_fonts();
        let (step, mask) = body_fit(&fonts, 1.0, "One\rTwo", 300, 200);
        assert_eq!(step, 0, "15 pt fits");
        let inked = rows(&mask);
        let gaps = inked.windows(2).filter(|w| w[1] > w[0] + 1).count();
        assert_eq!(gaps, 1, "two lines: {inked:?}");
    }

    #[test]
    fn body_text_fits_as_the_pages_scroll_height_does() {
        // Two 21 pt lines (15 pt × 1.4) fit a box 1 pt shorter (`scrollHeight <= clientHeight + 1`).
        let fonts = body_fonts();
        assert_eq!(body_fit(&fonts, 1.0, "One\rTwo", 300, 41).0, 0);
        assert_eq!(body_fit(&fonts, 1.0, "One\rTwo", 300, 40).0, 1);
        assert_eq!(body_fit(&fonts, 2.0, "One\rTwo", 600, 82).0, 0);
        assert_eq!(body_fit(&fonts, 2.0, "One\rTwo", 600, 79).0, 1);
    }

    #[test]
    fn the_fit_snaps_to_whole_css_px_as_blink_does() {
        // Blink snaps `scrollHeight` and `clientHeight` to whole CSS px (half up), so a step
        // within half a px of the box still fits, as the page picks it.
        let fonts = body_fonts();
        // Three 18.2 px lines at 13 pt are 54.6 → 55; a 107 px box @2 is 53.5 → 54: 55 ≤ 55.
        assert_eq!(body_fit(&fonts, 2.0, "One\rTwo\rThree", 600, 107).0, 1);
        // Two 21 px lines in an 81 px box @2 (40.5 → 41): 42 ≤ 42, so 15 pt.
        assert_eq!(body_fit(&fonts, 2.0, "One\rTwo", 600, 81).0, 0);
        // Two 18.2 px lines are 36.4 → 36; a 35 px box: 36 ≤ 36, so 13 pt.
        assert_eq!(body_fit(&fonts, 1.0, "One\rTwo", 300, 35).0, 1);
        assert_eq!(body_fit(&fonts, 1.0, "One\rTwo", 300, 34).0, 2);
    }

    #[test]
    fn body_lines_sit_where_the_pages_line_boxes_put_them() {
        // CSS: line n's box starts n line heights down, the font's ascent and descent centred in it.
        let fonts = body_fonts();
        // Three lines: 126 px at 15 pt, 109.2 at 13, 85.8 at 11.
        for (step, height) in [(0, 400), (1, 115), (2, 100)] {
            let (used, mask) = body_fit(&fonts, 2.0, "HH\rHH\rHH", 200, height);
            assert_eq!(used, step);
            let (points, lh) = BODY_STEPS[step];
            let line = points * lh * 2.0;
            let font = &fonts[step];
            // SAFETY: a live font.
            let (a, d) = unsafe { (2.0 * font.ascent(), 2.0 * font.descent()) };
            let inked = rows(&mask);
            let feet: Vec<usize> = inked
                .windows(2)
                .filter(|w| w[1] > w[0] + 1)
                .map(|w| w[0])
                .chain(inked.last().copied())
                .collect();
            for (n, &foot) in feet.iter().enumerate() {
                let baseline = n as f64 * line + (line - (a + d)) / 2.0 + a;
                let foot = foot as f64 + 1.0;
                assert!(
                    (foot - baseline).abs() <= 1.0,
                    "step {step} line {n}: {foot} vs {baseline}"
                );
            }
        }
    }

    #[test]
    fn body_text_shrinks_until_it_fits() {
        let fonts = body_fonts();
        let words = "light ".repeat(60);
        let (step, _) = body_fit(&fonts, 1.0, &words, 300, 400);
        assert_eq!(step, 0);
        let (step, mask) = body_fit(&fonts, 1.0, &words, 300, 120);
        assert!(step > 0, "shrinks");
        assert!(*rows(&mask).last().unwrap() < 120);
        let many = words.repeat(10);
        let (step, tall) = body_fit(&fonts, 1.0, &many, 300, 200);
        assert_eq!(step, BODY_STEPS.len() - 1, "clips at 11 pt");
        // A box whose foot crosses a line's glyphs shows the top of that line, as the page's
        // `overflow: hidden` does: the same lines, cut.
        let inked = rows(&tall);
        let cut = (40..150)
            .find(|&y| inked.contains(&(y - 1)) && inked.contains(&y))
            .expect("a row inside a line");
        let (step, short) = body_fit(&fonts, 1.0, &many, 300, cut);
        assert_eq!(step, BODY_STEPS.len() - 1);
        assert_eq!(short.alpha, tall.alpha[..300 * cut]);
        assert!(rows(&short).contains(&(cut - 1)));
    }

    #[test]
    fn semibold_inks_more_than_regular() {
        let sum = |m: &TextMask| m.alpha.iter().map(|&a| u64::from(a)).sum::<u64>();
        let set = |weight| {
            let font = ui_font(17.0, weight);
            let set = rasterize_line(
                &font,
                1.0,
                "Chromatron",
                Align::Left,
                Overflow::Clip,
                200,
                30,
            );
            sum(&set)
        };
        let (regular, medium, semibold) = (set(REGULAR), set(MEDIUM), set(SEMIBOLD));
        assert!(
            regular < medium && medium < semibold,
            "{regular} {medium} {semibold}"
        );
    }

    #[test]
    fn the_typesetter_measures_and_inks_at_its_scale() {
        let mut t = CaptionTypesetter::new(2.0);
        let narrow = t.measure("(won)", CaptionStyle::Badge);
        assert!(narrow > 20.0 && t.measure("(won) (won)", CaptionStyle::Badge) > narrow * 1.8);
        let line = CaptionLine {
            text: "Hint text".into(),
            rect: DeviceRect::new(10, 10, 410, 110),
            style: CaptionStyle::Body,
            alpha: 1.0,
            align: Align::Left,
        };
        let palette = halation::Theme::Dark.palette();
        let mut canvas = vec![0u32; 500 * 200];
        t.ink(
            std::slice::from_ref(&line),
            palette,
            &mut canvas,
            500,
            200,
            Clip::Whole,
        );
        assert_eq!(t.cached(), 1);
        let inked: Vec<usize> = (0..canvas.len()).filter(|&i| canvas[i] != 0).collect();
        assert!(!inked.is_empty());
        assert!(
            inked
                .iter()
                .all(|&i| (10..410).contains(&(i % 500)) && (10..110).contains(&(i / 500)))
        );
        t.ink(&[], palette, &mut canvas, 500, 200, Clip::Whole);
        assert_eq!(t.cached(), 0, "a frame without it drops it");
        t.set_scale(1.0);
        assert!(t.measure("(won)", CaptionStyle::Badge) < narrow * 0.6);
    }

    #[test]
    fn ink_sets_each_style_aligned_in_its_tone() {
        let mut t = CaptionTypesetter::new(1.0);
        let line = |text: &str, style, align, top| CaptionLine {
            text: text.into(),
            rect: DeviceRect::new(0, top, 200, top + 28),
            style,
            alpha: 1.0,
            align,
        };
        let lines = [
            line("(won)", CaptionStyle::Badge, Align::Right, 0),
            line("site", CaptionStyle::Credit, Align::Centre, 40),
            line("Levels", CaptionStyle::Pill, Align::Left, 80),
        ];
        let palette = halation::Theme::Dark.palette();
        let mut canvas = vec![0u32; 200 * 120];
        t.ink(&lines, palette, &mut canvas, 200, 120, Clip::Whole);
        let inked = |top: usize| -> Vec<usize> {
            (0..200)
                .filter(|&x| (top..top + 28).any(|y| canvas[y * 200 + x] != 0))
                .collect()
        };
        let (badge, credit, pill) = (inked(0), inked(40), inked(80));
        assert!(badge[0] > 150 && *badge.last().unwrap() >= 192, "{badge:?}");
        let middle = (credit[0] + credit.last().unwrap()) / 2;
        assert!((95..=105).contains(&middle), "{credit:?}");
        assert!(pill[0] < 4, "{pill:?}");
        // The fullest pixel of each is its tone: muted for the badge and credit, primary for the pill.
        let fullest = |top: usize| {
            let row = &canvas[top * 200..(top + 28) * 200];
            *row.iter().max_by_key(|&&p| (p >> 8) & 0xff).unwrap()
        };
        let rgb = |c: Rgb| (u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2);
        assert_eq!(fullest(0), rgb(palette.text.muted));
        assert_eq!(fullest(40), rgb(palette.text.muted));
        assert_eq!(fullest(80), rgb(palette.text.primary));
    }

    #[test]
    fn a_warm_ink_sets_nothing_again() {
        let mut t = CaptionTypesetter::new(1.0);
        let line = CaptionLine {
            text: "Level name".into(),
            rect: DeviceRect::new(0, 0, 200, 22),
            style: CaptionStyle::Title,
            alpha: 1.0,
            align: Align::Left,
        };
        let palette = halation::Theme::Dark.palette();
        let mut canvas = vec![0u32; 200 * 22];
        t.ink(
            std::slice::from_ref(&line),
            palette,
            &mut canvas,
            200,
            22,
            Clip::Whole,
        );
        let first = t.sets();
        t.ink(
            std::slice::from_ref(&line),
            palette,
            &mut canvas,
            200,
            22,
            Clip::Whole,
        );
        assert_eq!((first, t.sets(), t.cached()), (1, 1, 1));
        let aligned = CaptionLine {
            align: Align::Right,
            ..line
        };
        t.ink(
            std::slice::from_ref(&aligned),
            palette,
            &mut canvas,
            200,
            22,
            Clip::Whole,
        );
        assert_eq!(t.sets(), 2, "another alignment is another mask");
    }
}
