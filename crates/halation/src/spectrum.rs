//! `spectrum`: every colour token of a theme (spec §7.1–§7.4). All drawing takes its colours
//! from a `Palette`; geometry never holds a literal colour. Phase 1 has the dark theme only.

use lumen::{BLUE, Colour, GREEN, RED};

/// An sRGB colour as code values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A straight-alpha sRGB colour, components 0..=1 (code value / 255), as `etch` blends it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
    pub const fn alpha(self, a: f32) -> Rgba {
        Rgba {
            r: self.0 as f32 / 255.0,
            g: self.1 as f32 / 255.0,
            b: self.2 as f32 / 255.0,
            a,
        }
    }
    pub const fn opaque(self) -> Rgba {
        self.alpha(1.0)
    }
}

impl Rgba {
    pub const CLEAR: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const fn with_alpha(self, a: f32) -> Rgba {
        Rgba { a, ..self }
    }
}

/// A token used at many alphas (`fg(α)`, `rim(α)`, `glass(α)`, `hollow(α)`): `rgb` at
/// `clamp(α·scale + add, 0, 1)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub rgb: Rgb,
    pub scale: f32,
    pub add: f32,
}

impl Tone {
    pub const fn plain(rgb: Rgb) -> Tone {
        Tone {
            rgb,
            scale: 1.0,
            add: 0.0,
        }
    }
    pub fn at(&self, alpha: f32) -> Rgba {
        self.rgb
            .alpha((alpha * self.scale + self.add).clamp(0.0, 1.0))
    }
}

/// The appearance the modern look follows. Phase 5 adds `Light`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Dark,
}

impl Theme {
    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Dark => &Palette::DARK,
        }
    }
}

/// How beams and lights reach the picture (§8). Phase 5 adds `Over`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightMode {
    /// Beams add into light tiles, which glow, bloom and add onto `base`.
    Additive,
}

/// Raised wall slabs (§7.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallTones {
    pub top: Rgb,
    pub bottom: Rgb,
    pub highlight: Rgba,
    pub shade: Rgba,
    pub rim: Rgba,
    pub hatch: Rgba,
    pub shadow: Rgba,
}

/// Caption colours (§7.1; typeset from phase 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextTones {
    pub primary: Rgb,
    pub body: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
}

/// Every colour token for one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub light_mode: LightMode,
    /// The page's ink.
    pub ink: Rgb,
    /// The backdrop's radial lift, centred on the board: these stops at 0 and 0.45, then transparent ink at 0.55 of the canvas diagonal.
    pub lift: [Rgba; 2],
    /// The vignette: transparent at 0.25 of the diagonal, this at 0.62, centred on the canvas.
    pub vignette: Rgba,
    /// The grain overlay's opacity.
    pub grain: f32,
    pub red: Rgb,
    pub green: Rgb,
    pub blue: Rgb,
    pub core_red: Rgb,
    pub core_green: Rgb,
    pub core_blue: Rgb,
    pub white: Rgb,
    /// Bright bars, sheens and ticks.
    pub fg: Tone,
    pub rim: Tone,
    pub glass: Tone,
    /// The tile glass's far stop.
    pub glass_deep: Rgb,
    /// Dark cores: tangler, teleporter, target disc, the mirror's matte side.
    pub hollow: Tone,
    pub grid_line: Rgb,
    /// Grid line alphas at 0, 0.65 and 1 of 0.75 × the board diagonal.
    pub grid_line_alpha: [f32; 3],
    pub grid_dot: Rgb,
    /// Grid dot alphas at the centre and at 0.75 × the board diagonal.
    pub grid_dot_alpha: [f32; 2],
    pub wall: WallTones,
    /// The laser capsule's and the gate plate's metal: three gradient stops.
    pub metal: [Rgb; 3],
    /// The laser and gate aperture disc.
    pub aperture: Rgb,
    pub mirror_tint: Rgb,
    pub bender_tint: Rgb,
    /// The prism's glass stops after `fg(0.40)`: violet, cyan.
    pub prism_tints: [Rgba; 2],
    pub conduit_rail: Rgba,
    pub teleport_ice: Rgb,
    /// The complementor's back and front halves.
    pub complement: [Rgb; 2],
    pub gate_rim: Rgba,
    pub gate_channel: Rgba,
    /// The mirror's faint light line.
    pub mirror_glint: Rgba,
    /// The far stop of the starburst's centre glint.
    pub star_glint: Rgb,
    pub text: TextTones,
    pub panel_fill: Rgba,
    /// The panel rim's `rim` alphas, top and bottom.
    pub panel_rim_alpha: [f32; 2],
    pub sheet_fill: Rgba,
    pub sheet_shadow: Rgba,
    pub scrim: Rgba,
    pub pill_fill: Rgba,
    pub pill_rim: Rgba,
    /// A tray well's fill (spec §7.5).
    pub well_fill: Rgba,
    /// A tray well's top shade (spec §7.5).
    pub well_shade: Rgba,
    /// The dragged piece's shadow (spec §7.5).
    pub drag_shadow: Rgba,
    /// A level chip's digit: locked, open, solved or current (spec §7.5).
    pub chip_digit: [Rgba; 3],
    /// A locked chip's padlock (spec §7.5).
    pub padlock: Rgba,
    /// The current chip's fill (spec §7.5).
    pub current_fill: Rgba,
    /// The current chip's glow (spec §7.5).
    pub current_glow: Rgba,
    /// The current chip's rim (spec §7.5).
    pub current_rim: Rgba,
    /// A solved chip's dot (spec §7.5).
    pub solved_dot: Rgb,
    /// The title letter's halo (spec §7.5).
    pub letter_halo: Rgb,
    /// The title letter's glint stops (spec §7.5).
    pub letter_glint: [Rgb; 2],
    /// The title letter's core (spec §7.5).
    pub letter_core: Rgb,
    /// The Next button's fill stops, top and bottom (spec §7.5).
    pub next_fill: [Rgba; 2],
    /// The Next button's rim (spec §7.5).
    pub next_rim: Rgba,
    /// The Next button's label (spec §7.5).
    pub next_ink: Rgba,
}

impl Palette {
    pub const DARK: Palette = Palette {
        light_mode: LightMode::Additive,
        ink: Rgb::hex(0x06070b),
        lift: [Rgb(30, 38, 72).alpha(0.55), Rgb(16, 20, 40).alpha(0.35)],
        vignette: Rgb(0, 0, 2).alpha(0.75),
        grain: 0.04,
        red: Rgb(255, 34, 52),
        green: Rgb(24, 255, 118),
        blue: Rgb(40, 96, 255),
        core_red: Rgb(255, 200, 196),
        core_green: Rgb(210, 255, 226),
        core_blue: Rgb(214, 226, 255),
        white: Rgb(255, 255, 255),
        fg: Tone::plain(Rgb(255, 255, 255)),
        rim: Tone::plain(Rgb(220, 230, 255)),
        glass: Tone::plain(Rgb(170, 190, 255)),
        glass_deep: Rgb(120, 140, 220),
        hollow: Tone::plain(Rgb(4, 5, 9)),
        grid_line: Rgb(150, 170, 255),
        grid_line_alpha: [0.26, 0.20, 0.07],
        grid_dot: Rgb(205, 218, 255),
        grid_dot_alpha: [0.50, 0.18],
        wall: WallTones {
            top: Rgb::hex(0x323a50),
            bottom: Rgb::hex(0x191d29),
            highlight: Rgb(232, 238, 255).alpha(0.55),
            shade: Rgb(0, 0, 0).alpha(0.45),
            rim: Rgb(215, 225, 255).alpha(0.34),
            hatch: Rgb(220, 230, 255).alpha(0.075),
            shadow: Rgb(0, 0, 0).alpha(0.85),
        },
        metal: [Rgb::hex(0x3a4152), Rgb::hex(0x1a1e29), Rgb::hex(0x0b0d13)],
        aperture: Rgb::hex(0x05060a),
        mirror_tint: Rgb(190, 210, 255),
        bender_tint: Rgb(190, 170, 255),
        prism_tints: [
            Rgb(170, 160, 255).alpha(0.16),
            Rgb(120, 220, 255).alpha(0.12),
        ],
        conduit_rail: Rgb(200, 215, 255).alpha(0.35),
        teleport_ice: Rgb(220, 232, 255),
        complement: [Rgb::hex(0x06070b), Rgb::hex(0xd8def0)],
        gate_rim: Rgb(230, 236, 255).alpha(0.28),
        gate_channel: Rgb(200, 215, 255).alpha(0.4),
        mirror_glint: Rgb(170, 190, 255).alpha(0.22),
        star_glint: Rgb(200, 215, 255),
        text: TextTones {
            primary: Rgb::hex(0xe8edff),
            body: Rgb::hex(0xc3cbe6),
            muted: Rgb::hex(0x8a94b8),
            accent: Rgb::hex(0x8ab4ff),
        },
        panel_fill: Rgb(14, 17, 30).alpha(0.78),
        panel_rim_alpha: [0.22, 0.05],
        sheet_fill: Rgb(14, 17, 30).alpha(0.96),
        sheet_shadow: Rgb(0, 0, 0).alpha(0.6),
        scrim: Rgb::hex(0x06070b).alpha(0.6),
        pill_fill: Rgb(170, 190, 255).alpha(0.10),
        pill_rim: Rgb(220, 230, 255).alpha(0.28),
        well_fill: Rgb::hex(0x06070b).alpha(0.5),
        well_shade: Rgb::hex(0x06070b).alpha(0.6),
        drag_shadow: Rgb(0, 0, 0).alpha(0.75),
        chip_digit: [
            Rgb(138, 148, 184).alpha(0.35),
            Rgb(195, 203, 230).alpha(0.8),
            Rgb::hex(0xe8edff).alpha(1.0),
        ],
        padlock: Rgb(138, 148, 184).alpha(0.5),
        current_fill: Rgb(255, 255, 255).alpha(0.12),
        current_glow: Rgb(255, 255, 255).alpha(0.25),
        current_rim: Rgb(255, 255, 255).alpha(0.9),
        solved_dot: Rgb(255, 255, 255),
        letter_halo: Rgb(150, 175, 255),
        letter_glint: [Rgb(200, 220, 255), Rgb(160, 190, 255)],
        letter_core: Rgb(190, 210, 255),
        next_fill: [
            Rgb(170, 190, 255).alpha(0.20),
            Rgb(120, 140, 220).alpha(0.08),
        ],
        next_rim: Rgb(220, 230, 255).alpha(0.5),
        next_ink: Rgb(255, 255, 255).alpha(0.97),
    };

    /// The colour of one lumen bit (`RED` 4, `GREEN` 2, `BLUE` 1); white for anything else.
    pub fn channel(&self, bit: Colour) -> Rgb {
        match bit {
            RED => self.red,
            GREEN => self.green,
            BLUE => self.blue,
            _ => self.white,
        }
    }

    /// A beam mask's colour: the channels added and clamped, then scaled so the brightest
    /// component is 255 (the trailer's `maskColor`). Mask 0 is black.
    pub fn mix(&self, mask: Colour) -> Rgb {
        let mut sum = [0u32; 3];
        for bit in [RED, GREEN, BLUE] {
            if mask & bit != 0 {
                let c = self.channel(bit);
                for (s, v) in sum.iter_mut().zip([c.0, c.1, c.2]) {
                    *s = (*s + u32::from(v)).min(255);
                }
            }
        }
        let top = sum.into_iter().max().unwrap_or(0).max(1);
        let scale = |v: u32| (v * 255 / top) as u8;
        Rgb(scale(sum[0]), scale(sum[1]), scale(sum[2]))
    }

    /// The glow colour of a mask (dark: the mix).
    pub fn halo(&self, mask: Colour) -> Rgb {
        self.mix(mask)
    }

    /// A beam's core: a single channel's own core colour, white for mixes.
    pub fn core(&self, mask: Colour) -> Rgb {
        match mask {
            RED => self.core_red,
            GREEN => self.core_green,
            BLUE => self.core_blue,
            _ => self.white,
        }
    }

    /// This palette with every colour replaced by `rgb` (alphas and rules kept): the sentinel the
    /// "colours come only from the palette" tests draw with.
    pub fn recoloured(&self, rgb: Rgb) -> Palette {
        let c = |_: Rgb| rgb;
        let a = |x: Rgba| rgb.alpha(x.a);
        let t = |x: Tone| Tone { rgb, ..x };
        let p = self;
        Palette {
            light_mode: p.light_mode,
            ink: c(p.ink),
            lift: p.lift.map(a),
            vignette: a(p.vignette),
            grain: p.grain,
            red: rgb,
            green: rgb,
            blue: rgb,
            core_red: rgb,
            core_green: rgb,
            core_blue: rgb,
            white: rgb,
            fg: t(p.fg),
            rim: t(p.rim),
            glass: t(p.glass),
            glass_deep: rgb,
            hollow: t(p.hollow),
            grid_line: rgb,
            grid_line_alpha: p.grid_line_alpha,
            grid_dot: rgb,
            grid_dot_alpha: p.grid_dot_alpha,
            wall: WallTones {
                top: rgb,
                bottom: rgb,
                highlight: a(p.wall.highlight),
                shade: a(p.wall.shade),
                rim: a(p.wall.rim),
                hatch: a(p.wall.hatch),
                shadow: a(p.wall.shadow),
            },
            metal: [rgb; 3],
            aperture: rgb,
            mirror_tint: rgb,
            bender_tint: rgb,
            prism_tints: p.prism_tints.map(a),
            conduit_rail: a(p.conduit_rail),
            teleport_ice: rgb,
            complement: [rgb; 2],
            gate_rim: a(p.gate_rim),
            gate_channel: a(p.gate_channel),
            mirror_glint: a(p.mirror_glint),
            star_glint: rgb,
            text: TextTones {
                primary: rgb,
                body: rgb,
                muted: rgb,
                accent: rgb,
            },
            panel_fill: a(p.panel_fill),
            panel_rim_alpha: p.panel_rim_alpha,
            sheet_fill: a(p.sheet_fill),
            sheet_shadow: a(p.sheet_shadow),
            scrim: a(p.scrim),
            pill_fill: a(p.pill_fill),
            pill_rim: a(p.pill_rim),
            well_fill: a(p.well_fill),
            well_shade: a(p.well_shade),
            drag_shadow: a(p.drag_shadow),
            chip_digit: p.chip_digit.map(a),
            padlock: a(p.padlock),
            current_fill: a(p.current_fill),
            current_glow: a(p.current_glow),
            current_rim: a(p.current_rim),
            solved_dot: c(p.solved_dot),
            letter_halo: c(p.letter_halo),
            letter_glint: p.letter_glint.map(c),
            letter_core: c(p.letter_core),
            next_fill: p.next_fill.map(a),
            next_rim: a(p.next_rim),
            next_ink: a(p.next_ink),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen::{BLUE, GREEN, RED, WHITE};

    #[test]
    fn channels_follow_lumens_bits() {
        let p = &Palette::DARK;
        assert_eq!(p.channel(RED), Rgb(255, 34, 52));
        assert_eq!(p.channel(GREEN), Rgb(24, 255, 118));
        assert_eq!(p.channel(BLUE), Rgb(40, 96, 255));
    }

    #[test]
    fn mixes_add_then_scale_to_full_brightness() {
        let p = &Palette::DARK;
        assert_eq!(p.mix(RED), Rgb(255, 34, 52));
        assert_eq!(p.mix(RED | GREEN), Rgb(255, 255, 170));
        assert_eq!(p.mix(GREEN | BLUE), Rgb(64, 255, 255));
        assert_eq!(p.mix(WHITE), Rgb(255, 255, 255));
        assert_eq!(
            p.halo(RED | BLUE),
            p.mix(RED | BLUE),
            "dark halos are the mix"
        );
        assert_eq!(p.core(GREEN), Rgb(210, 255, 226));
        assert_eq!(p.core(RED | GREEN), Rgb(255, 255, 255));
    }

    #[test]
    fn tones_apply_their_alpha_rule() {
        assert_eq!(Palette::DARK.rim.at(0.35), Rgb(220, 230, 255).alpha(0.35));
        let light_rim = Tone {
            rgb: Rgb(27, 35, 64),
            scale: 1.3,
            add: 0.0,
        };
        assert!((light_rim.at(0.9).a - 1.0).abs() < 1e-6, "clamped to 1");
    }

    #[test]
    fn recolouring_reaches_every_colour() {
        let sentinel = Palette::DARK.recoloured(Rgb(255, 0, 255));
        assert_eq!(sentinel.mix(WHITE), Rgb(255, 0, 255));
        assert_eq!(
            sentinel.wall.rim,
            Rgb(255, 0, 255).alpha(Palette::DARK.wall.rim.a)
        );
        assert_eq!(sentinel.metal, [Rgb(255, 0, 255); 3]);
        assert_eq!(sentinel.fg.at(0.5), Rgb(255, 0, 255).alpha(0.5));
        assert_eq!(Theme::Dark.palette(), &Palette::DARK);
    }
}
