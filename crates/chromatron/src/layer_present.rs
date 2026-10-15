//! The R2 fallback (spec §14 "Present", §16): the canvas presented as `CALayer` contents from
//! two DeviceRGB-tagged IOSurfaces (a third when the compositor still holds the next one),
//! copying only the rows each surface lacks, instead of softbuffer's allocate-zero-and-copy
//! of the whole canvas on every present.

use halation::{DeviceRect, Patchwork};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2_core_foundation::{
    CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGColor, CGColorSpace};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceColorSpace,
    kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityTopLeft};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Damage rects a surface may owe before they merge.
pub const OWED: usize = 16;

/// The most surfaces a presenter keeps: two, and a third made once when the next one is
/// still in the compositor's hands (review I2).
pub const MOST_SURFACES: usize = 3;

/// What each surface lacks. A present copies into a back surface this frame's damage and
/// what it missed since it was last written, then that surface is the front.
pub struct DamageLedger {
    owed: [Patchwork<OWED>; MOST_SURFACES],
    front: usize,
}

impl Default for DamageLedger {
    fn default() -> Self {
        DamageLedger::new()
    }
}

impl DamageLedger {
    pub fn new() -> DamageLedger {
        DamageLedger {
            owed: [Patchwork::new(); MOST_SURFACES],
            front: 0,
        }
    }

    /// New surfaces (a resize), or a failed copy: every surface owes the whole canvas, and
    /// `front` is the one the layer shows, so the next present goes to another.
    pub fn reset(&mut self, canvas: DeviceRect, front: usize) {
        for owed in &mut self.owed {
            owed.clear();
            owed.add(canvas);
        }
        self.front = front;
    }

    /// The surface presented last.
    pub fn front(&self) -> usize {
        self.front
    }

    /// The rects to copy into `back` (a surface other than the front) for a present with
    /// `damage`; `back` is the front after it, and every other surface owes `damage`.
    pub fn present(&mut self, back: usize, damage: &[DeviceRect], copy: &mut Patchwork<OWED>) {
        copy.clear();
        for &r in self.owed[back].as_slice().iter().chain(damage) {
            copy.add(r);
        }
        for (k, owed) in self.owed.iter_mut().enumerate() {
            if k == back {
                owed.clear();
            } else {
                for &r in damage {
                    owed.add(r);
                }
            }
        }
        self.front = back;
    }
}

/// What a paint presents: `whole` (the canvas) when `everything` (classic, which presents its
/// whole canvas as it always has, and probe and snapshot runs); otherwise the modern `damage`,
/// or nothing when there is none and the layer is `current` (a redundant `Resized`, an expose).
pub fn presented<'a>(
    whole: &'a DeviceRect,
    everything: bool,
    damage: &'a [DeviceRect],
    current: bool,
) -> Option<&'a [DeviceRect]> {
    if everything {
        Some(std::slice::from_ref(whole))
    } else if damage.is_empty() && current {
        None
    } else {
        Some(damage)
    }
}

/// 'BGRA': the canvas's `0x00RRGGBB` words, little-endian; the alpha byte is set to 0xFF as
/// rows are copied, so CoreAnimation never reads the canvas's zero byte as transparency.
const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const OPAQUE: u32 = 0xff00_0000;

/// The DeviceRGB colour space as a property list: softbuffer's `CGImage` was DeviceRGB, which
/// Core Animation colour-matches; an untagged IOSurface would show in the display's own space
/// (more saturated on a P3 panel). Review I1.
fn device_rgb() -> Result<CFRetained<CFType>, String> {
    CGColorSpace::new_device_rgb()
        .and_then(|space| space.property_list())
        .ok_or_else(|| "cannot describe the DeviceRGB colour space".to_owned())
}

/// A `width × height` BGRA IOSurface whose rows hold at least `width` pixels, tagged with the
/// colour space `space` (a property list).
fn surface(
    width: usize,
    height: usize,
    space: &CFType,
) -> Result<CFRetained<IOSurfaceRef>, String> {
    let n = |v: i64| CFNumber::new_i64(v);
    let (w, h, bpe, fmt) = (n(width as i64), n(height as i64), n(4), n(i64::from(BGRA)));
    // SAFETY: immutable CFString constants.
    let keys = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    let values: [&CFType; 4] = [w.as_ref(), h.as_ref(), bpe.as_ref(), fmt.as_ref()];
    let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    // SAFETY: the dictionary holds IOSurface's documented keys with CFNumber values.
    let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }
        .ok_or_else(|| format!("cannot make a {width} × {height} IOSurface"))?;
    if surface.width() < width || surface.height() < height || surface.bytes_per_row() < width * 4 {
        return Err(format!("the IOSurface is smaller than {width} × {height}"));
    }
    // SAFETY: kIOSurfaceColorSpace takes a CGColorSpace property list. It is set as a value:
    // as a creation property IOSurface ignores it (softbuffer PR #329).
    unsafe { surface.set_value(kIOSurfaceColorSpace, space) };
    Ok(surface)
}

/// The surface locked for the closure, unlocked after it; `read_only` when only read.
fn locked<T>(
    surface: &IOSurfaceRef,
    read_only: bool,
    f: impl FnOnce(*mut u8, usize) -> T,
) -> Result<T, String> {
    let options = if read_only {
        IOSurfaceLockOptions::ReadOnly
    } else {
        IOSurfaceLockOptions::empty()
    };
    // SAFETY: a valid surface, no seed wanted.
    let status = unsafe { surface.lock(options, std::ptr::null_mut()) };
    if status != 0 {
        return Err(format!("cannot lock the IOSurface ({status})"));
    }
    let out = f(
        surface.base_address().as_ptr().cast::<u8>(),
        surface.bytes_per_row(),
    );
    // SAFETY: locked above with the same options.
    unsafe { surface.unlock(options, std::ptr::null_mut()) };
    Ok(out)
}

/// Copies `rects` of the `width × height` canvas into the surface, alpha set opaque.
fn copy_rects(
    surface: &IOSurfaceRef,
    canvas: &[u32],
    width: usize,
    height: usize,
    rects: &[DeviceRect],
) -> Result<(), String> {
    locked(surface, false, |base, stride| {
        for r in rects {
            let (left, right) = (r.left.max(0) as usize, (r.right.max(0) as usize).min(width));
            let (top, bottom) = (
                r.top.max(0) as usize,
                (r.bottom.max(0) as usize).min(height),
            );
            if left >= right {
                continue;
            }
            for y in top..bottom {
                let src = &canvas[y * width + left..y * width + right];
                // SAFETY: the surface is locked; `surface` checked that its rows hold `width`
                // pixels and that it has `height` of them; rows are 4-byte aligned.
                let dst = unsafe {
                    std::slice::from_raw_parts_mut(
                        base.add(y * stride + left * 4).cast::<u32>(),
                        right - left,
                    )
                };
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = s | OPAQUE;
                }
            }
        }
    })
}

/// Whether the surface holds the `width × height` canvas, alpha opaque.
fn holds(
    surface: &IOSurfaceRef,
    canvas: &[u32],
    width: usize,
    height: usize,
) -> Result<bool, String> {
    locked(surface, true, |base, stride| {
        (0..height).all(|y| {
            // SAFETY: as in `copy_rects`, read only.
            let row =
                unsafe { std::slice::from_raw_parts(base.add(y * stride).cast::<u32>(), width) };
            row.iter()
                .zip(&canvas[y * width..(y + 1) * width])
                .all(|(&d, &s)| d == s | OPAQUE)
        })
    })
}

/// Runs `f`'s layer changes in a transaction without implicit animations (a changed
/// `contents` or `frame` would otherwise fade or slide for a quarter second).
fn without_actions(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

/// The window's content as an opaque layer whose contents are one of two IOSurfaces, or three
/// when the compositor still holds the next one.
pub struct LayerPresenter {
    layer: Retained<CALayer>,
    /// The DeviceRGB property list every surface is tagged with.
    space: CFRetained<CFType>,
    /// The surfaces made for this size: the first two at a resize, the third (if ever) when
    /// both others were in use; always a prefix.
    surfaces: [Option<CFRetained<IOSurfaceRef>>; MOST_SURFACES],
    /// Whether the third surface was tried for this size (it is made at most once).
    grew: bool,
    width: usize,
    height: usize,
    scale: f64,
    points: (f64, f64),
    ledger: DamageLedger,
    copy: Patchwork<OWED>,
    /// Whether the layer shows the ledger's front surface, presented since the last resize
    /// with no failed copy after it.
    current: bool,
    /// Presents that wrote a surface the compositor still had in use (none was free).
    crowded: usize,
}

impl LayerPresenter {
    pub fn new(window: &Window) -> Result<LayerPresenter, String> {
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("not an AppKit window".to_owned());
        };
        // SAFETY: winit's AppKit handle is a live NSView on the main thread; making it
        // layer-backed and adding a sublayer is what softbuffer's CoreGraphics backend did.
        let root = unsafe {
            let view: &AnyObject = handle.ns_view.cast().as_ref();
            let _: () = msg_send![view, setWantsLayer: Bool::YES];
            let root: Option<Retained<CALayer>> = msg_send![view, layer];
            root.ok_or("the view has no layer")?
        };
        let layer = CALayer::new();
        without_actions(|| {
            root.addSublayer(&layer);
            layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
            layer.setGeometryFlipped(true);
            layer.setOpaque(true);
            // As softbuffer did: during a live resize the old picture stays put, unscaled.
            // SAFETY: an immutable CoreAnimation constant.
            layer.setContentsGravity(unsafe { kCAGravityTopLeft });
        });
        Ok(LayerPresenter {
            layer,
            space: device_rgb()?,
            surfaces: std::array::from_fn(|_| None),
            grew: false,
            width: 0,
            height: 0,
            scale: 0.0,
            points: (0.0, 0.0),
            ledger: DamageLedger::new(),
            copy: Patchwork::new(),
            current: false,
            crowded: 0,
        })
    }

    /// The colour the layer shows where its contents don't reach: the strip that a live resize
    /// or the fullscreen animation uncovers before the next present (final review M1). Without
    /// it that strip is black, which shows against a light page.
    pub fn set_ground(&self, [r, g, b]: [u8; 3]) {
        let rgba = [r, g, b, 255].map(|c| c as f64 / 255.0);
        let Some(space) = CGColorSpace::new_device_rgb() else {
            return;
        };
        // SAFETY: four components, the device RGB space's three and alpha.
        let Some(color) = (unsafe { CGColor::new(Some(&space), rgba.as_ptr()) }) else {
            return;
        };
        without_actions(|| self.layer.setBackgroundColor(Some(&color)));
    }

    /// How many surfaces this size has.
    fn count(&self) -> usize {
        self.surfaces.iter().take_while(|s| s.is_some()).count()
    }

    /// The whole canvas, as a rect.
    fn whole(&self) -> DeviceRect {
        DeviceRect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// The surface to present into: the next after the front that the compositor no longer
    /// uses; else a third, made once; else the next anyway (it may tear for a frame).
    fn back(&mut self) -> usize {
        let front = self.ledger.front();
        let n = self.count();
        for step in 1..n {
            let k = (front + step) % n;
            if self.surfaces[k].as_ref().is_some_and(|s| !s.is_in_use()) {
                return k;
            }
        }
        if n < MOST_SURFACES && !self.grew {
            self.grew = true;
            match surface(self.width, self.height, &self.space) {
                Ok(s) => {
                    self.surfaces[n] = Some(s);
                    return n;
                }
                Err(e) => log::warn!("{e}: presenting into a surface still in use"),
            }
        }
        self.crowded += 1;
        log::debug!("every surface but the front is in use: presenting into one anyway");
        (front + 1) % n
    }

    /// New surfaces for a canvas of `width × height` px; the layer covers `points` at `scale`.
    /// The same size again keeps the surfaces and what they hold.
    pub fn resize(
        &mut self,
        width: usize,
        height: usize,
        scale: f64,
        points: (f64, f64),
    ) -> Result<(), String> {
        if self.count() > 0
            && (self.width, self.height, self.scale, self.points) == (width, height, scale, points)
        {
            return Ok(());
        }
        // Until both new surfaces exist there are none (the layer keeps its old picture).
        self.surfaces = std::array::from_fn(|_| None);
        self.current = false;
        self.grew = false;
        let first = surface(width, height, &self.space)?;
        let second = surface(width, height, &self.space)?;
        self.surfaces[0] = Some(first);
        self.surfaces[1] = Some(second);
        (self.width, self.height, self.scale, self.points) = (width, height, scale, points);
        self.ledger.reset(self.whole(), 0);
        without_actions(|| {
            self.layer.setContentsScale(scale);
            self.layer.setFrame(CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(points.0, points.1),
            ));
        });
        Ok(())
    }

    /// Whether the layer shows a surface presented since the last resize: the canvas as it was
    /// then, so a present with no damage would change nothing.
    pub fn is_current(&self) -> bool {
        self.current
    }

    /// Copies what a back surface lacks from `canvas` (alpha set opaque) and shows it.
    pub fn present(&mut self, canvas: &[u32], damage: &[DeviceRect]) -> Result<(), String> {
        if self.count() == 0 {
            return Err("no surfaces yet".to_owned());
        }
        if canvas.len() < self.width * self.height {
            return Err("the canvas is smaller than the surfaces".to_owned());
        }
        let shown = self.ledger.front();
        let k = self.back();
        self.ledger.present(k, damage, &mut self.copy);
        let Some(surface) = &self.surfaces[k] else {
            return Err("no surface to present into".to_owned());
        };
        if let Err(e) = copy_rects(
            surface,
            canvas,
            self.width,
            self.height,
            self.copy.as_slice(),
        ) {
            // The ledger counted this copy as done: owe everything again, with the surface
            // still shown in front (review M1), and present at the next paint even without
            // damage.
            self.ledger.reset(self.whole(), shown);
            self.current = false;
            return Err(e);
        }
        let contents: &CFType = surface;
        // SAFETY: an IOSurface is valid `CALayer` contents.
        without_actions(|| unsafe { self.layer.setContents(Some(contents.as_ref())) });
        self.current = true;
        Ok(())
    }

    /// Whether the surface shown holds `canvas` (alpha opaque): the probe's check.
    pub fn shows(&self, canvas: &[u32]) -> Result<bool, String> {
        match &self.surfaces[self.ledger.front()] {
            Some(surface) if self.current && canvas.len() >= self.width * self.height => {
                holds(surface, canvas, self.width, self.height)
            }
            _ => Ok(false),
        }
    }

    /// The surfaces this size has and the bytes they take (review M3).
    pub fn memory(&self) -> (usize, usize) {
        let bytes = self.surfaces.iter().flatten().map(|s| s.alloc_size()).sum();
        (self.count(), bytes)
    }

    /// Presents that wrote a surface the compositor still had in use.
    pub fn crowded(&self) -> usize {
        self.crowded
    }
}

impl Drop for LayerPresenter {
    fn drop(&mut self) {
        self.layer.removeFromSuperlayer();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 11 review M1: a modern paint with no damage presents nothing once the layer shows
    /// the canvas; classic, probe and snapshot runs present the whole canvas every time.
    #[test]
    fn only_damage_is_presented_and_none_is_skipped() {
        let whole = DeviceRect::new(0, 0, 640, 480);
        let damage = [DeviceRect::new(1, 2, 3, 4)];
        assert_eq!(presented(&whole, false, &[], true), None);
        assert_eq!(
            presented(&whole, false, &[], false),
            Some(&[][..]),
            "after a resize the ledger's owed copy still goes"
        );
        assert_eq!(presented(&whole, false, &damage, true), Some(&damage[..]));
        for current in [false, true] {
            assert_eq!(presented(&whole, true, &[], current), Some(&[whole][..]));
            assert_eq!(
                presented(&whole, true, &damage, current),
                Some(&[whole][..])
            );
        }
    }

    /// The real path on real IOSurfaces (no window): the ledger's copies, alpha set opaque,
    /// leave each surface presented holding the canvas, after a full first copy and random
    /// damage after it.
    #[test]
    fn iosurfaces_hold_the_canvas_opaque() {
        let (w, h) = (97usize, 61usize);
        let mut canvas: Vec<u32> = (0..(w * h) as u32)
            .map(|i| i.wrapping_mul(2_654_435_761) >> 8)
            .collect();
        let space = device_rgb().unwrap();
        let surfaces = [
            surface(w, h, &space).unwrap(),
            surface(w, h, &space).unwrap(),
        ];
        // I1: each surface carries softbuffer's DeviceRGB tag, set as a value (read back).
        for s in &surfaces {
            // SAFETY: an immutable CFString constant.
            let tag = s.value(unsafe { kIOSurfaceColorSpace });
            assert_eq!(tag.as_deref(), Some(&*space), "tagged DeviceRGB");
        }
        let mut ledger = DamageLedger::new();
        ledger.reset(DeviceRect::new(0, 0, w as i32, h as i32), 0);
        let mut copy = Patchwork::new();
        for frame in 0..40usize {
            let r = DeviceRect::new(
                (frame * 7 % w) as i32,
                (frame * 5 % h) as i32,
                (frame * 7 % w + 13).min(w) as i32,
                (frame * 5 % h + 9).min(h) as i32,
            );
            for y in r.top as usize..r.bottom as usize {
                for x in r.left as usize..r.right as usize {
                    canvas[y * w + x] = 0x00ff_ffff & (frame as u32 * 0x0001_0203 + x as u32);
                }
            }
            let k = 1 - ledger.front();
            ledger.present(k, &[r], &mut copy);
            copy_rects(&surfaces[k], &canvas, w, h, copy.as_slice()).unwrap();
            assert!(holds(&surfaces[k], &canvas, w, h).unwrap(), "frame {frame}");
        }
        // The check is not vacuous: another canvas does not match.
        let other = vec![0u32; w * h];
        assert!(!holds(&surfaces[0], &other, w, h).unwrap());
    }

    /// Two, then three "surfaces" against a canvas that changes in random rects, each present
    /// going to a random surface other than the front one (as when the next is in use): every
    /// presented surface equals the canvas.
    #[test]
    fn every_presented_surface_matches_the_canvas() {
        for n in 2..=MOST_SURFACES {
            presents_match(n);
        }
    }

    fn presents_match(n: usize) {
        let (w, h) = (64usize, 48usize);
        let mut canvas = vec![0u32; w * h];
        let mut surfaces: Vec<Vec<u32>> = (0..n).map(|i| vec![i as u32 + 1; w * h]).collect();
        let mut ledger = DamageLedger::new();
        ledger.reset(DeviceRect::new(0, 0, w as i32, h as i32), 0);
        let mut seed = 0x2545_f491_u32;
        let mut next = |m: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % m
        };
        for frame in 1..300u32 {
            let mut damage = Vec::new();
            for _ in 0..next(4) {
                let (x, y) = (next(w as u32) as i32, next(h as u32) as i32);
                let r = DeviceRect::new(
                    x,
                    y,
                    (x + 1 + next(20) as i32).min(w as i32),
                    (y + 1 + next(20) as i32).min(h as i32),
                );
                for yy in r.top..r.bottom {
                    for xx in r.left..r.right {
                        canvas[yy as usize * w + xx as usize] = frame;
                    }
                }
                damage.push(r);
            }
            let mut copy = Patchwork::new();
            let k = (ledger.front() + 1 + next(n as u32 - 1) as usize) % n;
            ledger.present(k, &damage, &mut copy);
            assert_eq!(ledger.front(), k);
            for r in copy.as_slice() {
                for yy in r.top..r.bottom {
                    let row = yy as usize * w;
                    surfaces[k][row + r.left as usize..row + r.right as usize]
                        .copy_from_slice(&canvas[row + r.left as usize..row + r.right as usize]);
                }
            }
            assert!(surfaces[k] == canvas, "{n} surfaces, frame {frame}");
        }
    }

    /// Review M1: a failed copy owes everything again, with the surface still shown in front,
    /// so the next present goes to another one.
    #[test]
    fn a_reset_keeps_the_shown_surface_in_front() {
        let canvas = DeviceRect::new(0, 0, 8, 8);
        let mut ledger = DamageLedger::new();
        ledger.reset(canvas, 0);
        let mut copy = Patchwork::new();
        ledger.present(1, &[DeviceRect::new(0, 0, 2, 2)], &mut copy);
        ledger.present(0, &[], &mut copy);
        // A copy into 1 failed after the ledger had made it the front: 0 is still shown.
        ledger.present(1, &[], &mut copy);
        ledger.reset(canvas, 0);
        assert_eq!(ledger.front(), 0);
        for k in [1, 2] {
            ledger.present(k, &[], &mut copy);
            assert_eq!(
                copy.as_slice(),
                &[canvas][..],
                "surface {k} owes everything"
            );
        }
    }
}
