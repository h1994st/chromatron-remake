//! `easel`: the device-pixel canvas the look draws on (spec §5.2, §6).

/// Safe-area insets in pt (notches, home indicator), taken off every side before layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// A canvas of `width × height` device px at `px_per_pt` device px per pt (CSS px on the web,
/// AppKit points on macOS). `touch` picks touch sizes (the tray and the lift).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Easel {
    pub width: u32,
    pub height: u32,
    pub px_per_pt: f32,
    pub safe: Insets,
    pub touch: bool,
}

impl Easel {
    /// At least 1 × 1 px; a scale that is not a positive finite number counts as 1.
    pub fn new(width: u32, height: u32, px_per_pt: f32) -> Easel {
        let px_per_pt = if px_per_pt.is_finite() && px_per_pt > 0.0 {
            px_per_pt
        } else {
            1.0
        };
        Easel {
            width: width.max(1),
            height: height.max(1),
            px_per_pt,
            safe: Insets::default(),
            touch: false,
        }
    }

    /// Insets that are not positive finite numbers count as 0.
    pub fn with_safe(self, safe: Insets) -> Easel {
        let clean = |v: f32| if v.is_finite() && v > 0.0 { v } else { 0.0 };
        Easel {
            safe: Insets {
                top: clean(safe.top),
                right: clean(safe.right),
                bottom: clean(safe.bottom),
                left: clean(safe.left),
            },
            ..self
        }
    }

    pub fn with_touch(self, touch: bool) -> Easel {
        Easel { touch, ..self }
    }

    /// The canvas in pt after the safe area: spec §6's `W × H` (never negative).
    pub fn room(&self) -> (f32, f32) {
        let w = self.width as f32 / self.px_per_pt - self.safe.left - self.safe.right;
        let h = self.height as f32 / self.px_per_pt - self.safe.top - self.safe.bottom;
        (w.max(0.0), h.max(0.0))
    }

    /// Device x of pt `pt`, measured from the safe area's left edge.
    pub fn px_x(&self, pt: f32) -> f32 {
        (self.safe.left + pt) * self.px_per_pt
    }

    /// Device y of pt `pt`, measured from the safe area's top edge.
    pub fn px_y(&self, pt: f32) -> f32 {
        (self.safe.top + pt) * self.px_per_pt
    }

    /// `WxH@S` (pt and scale, as the CLI and `just shot-modern` take it): `1280x800@2` is
    /// 2560 × 1600 device px.
    pub fn parse(arg: &str) -> Option<Easel> {
        let (size, scale) = arg.split_once('@')?;
        let (w, h) = size.split_once('x')?;
        let (w, h, scale): (f32, f32, f32) =
            (w.parse().ok()?, h.parse().ok()?, scale.parse().ok()?);
        if !(w > 0.0 && h > 0.0 && scale > 0.0 && scale.is_finite()) {
            return None;
        }
        Some(Easel::new(
            (w * scale).round() as u32,
            (h * scale).round() as u32,
            scale,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_room_is_the_canvas_in_pt_less_the_safe_area() {
        let easel = Easel::new(2560, 1600, 2.0);
        assert_eq!(easel.room(), (1280.0, 800.0));
        let notched = Easel::new(1688, 780, 2.0).with_safe(Insets {
            top: 0.0,
            right: 47.0,
            bottom: 21.0,
            left: 47.0,
        });
        assert_eq!(notched.room(), (750.0, 369.0));
        assert_eq!(notched.px_x(0.0), 94.0, "pt 0 is the safe area's left edge");
        assert_eq!(notched.px_y(10.0), 20.0);
    }

    #[test]
    fn broken_scales_and_sizes_are_made_safe() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(Easel::new(10, 10, scale).px_per_pt, 1.0, "{scale}");
        }
        let easel = Easel::new(0, 0, 1.0);
        assert_eq!((easel.width, easel.height), (1, 1));
        let squeezed = Easel::new(100, 100, 1.0).with_safe(Insets {
            top: 80.0,
            right: 80.0,
            bottom: 80.0,
            left: 80.0,
        });
        assert_eq!(squeezed.room(), (0.0, 0.0));
    }

    #[test]
    fn easels_parse_from_the_cli_form() {
        let easel = Easel::parse("1280x800@2").unwrap();
        assert_eq!(
            (easel.width, easel.height, easel.px_per_pt),
            (2560, 1600, 2.0)
        );
        assert_eq!(Easel::parse("390x740@3").unwrap().width, 1170);
        for bad in ["", "1280x800", "1280@2", "ax800@2", "1280x800@0"] {
            assert!(Easel::parse(bad).is_none(), "{bad}");
        }
    }
}
