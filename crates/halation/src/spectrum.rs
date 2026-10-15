//! `spectrum`: every colour token of a theme (spec §7.1–§7.4). All drawing takes its colours
//! from a `Palette`; geometry never holds a literal colour. Two themes: `Palette::DARK` and
//! `Palette::LIGHT` (phase 5).

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

/// An alpha growing with a highlight `h` (0–1): `tone` at `base + gain·h` (spec §7.2 tiles,
/// §7.5 chips). Computed as `tone.at(base + gain·h)`, the phase-1 recipe's own operations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    pub tone: Tone,
    pub base: f32,
    pub gain: f32,
}

impl Ramp {
    pub const fn new(tone: Tone, base: f32, gain: f32) -> Ramp {
        Ramp { tone, base, gain }
    }
    pub fn at(&self, h: f32) -> Rgba {
        self.tone.at(self.base + self.gain * h)
    }
}

/// A glass face's fill (spec §7.2 tile, §7.5 chip): `near` at its top-left corner, `far` at
/// `reach` of the face ((1, 1) is the far corner, the dark recipe; (0.4, 1) the comps' light one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glaze {
    pub near: Ramp,
    pub far: Ramp,
    pub reach: (f32, f32),
}

/// A Canvas shadow in `colour`: `shadowBlur` `blur_pt` (so σ = blur / 2), `drop_pt` down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub colour: Rgba,
    pub blur_pt: f32,
    pub drop_pt: f32,
}

/// The light board's panel (spec §7.2): a radial fill over 0.75 × the board's width through
/// `fill` at 0, 0.6 and 1; a 1-px rim; its shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardPanel {
    pub fill: [Rgb; 3],
    pub rim: Rgba,
    pub shadow: Shadow,
}

/// A lit target's light (spec §7.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitTone {
    /// A halo under the arcs, (α, radius in u): the light theme's.
    pub halo: Option<(f32, f32)>,
    /// The received channels' arcs' width (u).
    pub arc_width: f32,
    /// The core: white at its centre, the mix at α `core_mid` at 0.35, the mix at α `core_end`
    /// at `core_radius` (u).
    pub core_mid: f32,
    pub core_end: f32,
    pub core_radius: f32,
    /// The yellow target's middle stop, where it has its own (light: `rgb(255,230,128)`).
    pub yellow_mid: Option<Rgb>,
    /// The ring at r 0.186u: its α and width (u).
    pub ring_alpha: f32,
    pub ring_width: f32,
    /// A spoilt black target's shade, (colour at its centre → clear, radius in u): the light
    /// theme's, whose white disc would otherwise read half lit.
    pub spoilt_shade: Option<(Rgba, f32)>,
}

/// A chip's rim (spec §7.5): `tone` at `alpha` (locked, otherwise) times each vertical stop's
/// `profile`, `width_pt` wide (none: `max(1 px, 0.028·size)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipRim {
    pub tone: Tone,
    pub alpha: [f32; 2],
    pub profile: [f32; 3],
    pub width_pt: Option<f32>,
}

/// The appearance the modern look follows: the system's, live (spec §11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Dark, Theme::Light];

    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Dark => &Palette::DARK,
            Theme::Light => &Palette::LIGHT,
        }
    }
}

/// How beams and lights reach the picture (§8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightMode {
    /// Beams add into light tiles, which glow, bloom and add onto `base`.
    Additive,
    /// Beams, mix dots and piece lights are painted in normal blending into a `gouache::Wash`
    /// composed over `base` (§7.3 "Light", ruling L3).
    Over,
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
    /// The page's ground, top and bottom (dark: the ink, flat).
    pub ground: [Rgb; 2],
    /// The backdrop's radial lift, centred on the board: these stops at 0 and 0.45, then transparent ink at 0.55 of the canvas diagonal.
    pub lift: [Rgba; 2],
    /// The lift's transparent end stop at 0.55 of the diagonal.
    pub lift_end: Rgba,
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
    /// Two channels together in one beam or piece: their own saturated colours, not the
    /// channels' sum (which reads as cream, lavender and ice).
    pub yellow: Rgb,
    pub cyan: Rgb,
    pub magenta: Rgb,
    pub core_yellow: Rgb,
    pub core_cyan: Rgb,
    pub core_magenta: Rgb,
    pub white: Rgb,
    /// The glow (halo) colour of white light (spec §7.1 "Halo": dark white, light blue-grey).
    pub halo_white: Rgb,
    /// Bright bars, sheens and ticks.
    pub fg: Tone,
    pub rim: Tone,
    pub glass: Tone,
    /// Dark cores: tangler, teleporter, target disc, the mirror's matte side.
    pub hollow: Tone,
    /// The glass tile under a piece (spec §7.2): its fill, its rim's three vertical stops, and
    /// (light) its shadow's colour; σ 0.04u, 0.03u down (masonry).
    pub tile: Glaze,
    pub tile_rim: [Ramp; 3],
    pub tile_shadow: Option<Ramp>,
    /// A level chip's fill and rim (spec §7.5).
    pub chip: Glaze,
    pub chip_rim: ChipRim,
    pub grid_line: Rgb,
    /// Grid line alphas at 0, 0.65 and 1 of 0.75 × the board diagonal.
    pub grid_line_alpha: [f32; 3],
    pub grid_dot: Rgb,
    /// Grid dot alphas at the centre and at 0.75 × the board diagonal.
    pub grid_dot_alpha: [f32; 2],
    pub wall: WallTones,
    /// The laser capsule's metal: three gradient stops.
    pub metal: [Rgb; 3],
    /// The logic gate's plate: three gradient stops (dark: the metal).
    pub gate_metal: [Rgb; 3],
    /// The laser and gate aperture disc.
    pub aperture: Rgb,
    /// The mirror's and the bender's glass: the slab's two stops across it.
    pub mirror_glass: [Rgba; 2],
    pub bender_glass: [Rgba; 2],
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
    /// A lit target's light (spec §7.4).
    pub lit: LitTone,
    pub text: TextTones,
    pub panel_fill: Rgba,
    /// The panel rim, top and bottom.
    pub panel_rim: [Rgba; 2],
    /// The card's and the tray's shadow (light; baked into the page, ruling L7).
    pub panel_shadow: Option<Shadow>,
    /// The board's panel (light, spec §7.2).
    pub board_panel: Option<BoardPanel>,
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
    /// Next ▶'s shadow (light).
    pub next_shadow: Option<Shadow>,
    /// Ink lettering's core line (spec §10, ruling L6).
    pub ink_core: Rgb,
}

impl Palette {
    pub const DARK: Palette = Palette {
        light_mode: LightMode::Additive,
        ink: Rgb::hex(0x06070b),
        ground: [Rgb::hex(0x06070b), Rgb::hex(0x06070b)],
        lift: [Rgb(30, 38, 72).alpha(0.55), Rgb(16, 20, 40).alpha(0.35)],
        lift_end: Rgb::hex(0x06070b).alpha(0.0),
        vignette: Rgb(0, 0, 2).alpha(0.75),
        grain: 0.04,
        red: Rgb(255, 34, 52),
        green: Rgb(24, 255, 118),
        blue: Rgb(40, 96, 255),
        core_red: Rgb(255, 200, 196),
        core_green: Rgb(210, 255, 226),
        core_blue: Rgb(214, 226, 255),
        yellow: Rgb(255, 214, 40),
        cyan: Rgb(30, 225, 255),
        magenta: Rgb(255, 50, 215),
        core_yellow: Rgb(255, 246, 200),
        core_cyan: Rgb(205, 248, 255),
        core_magenta: Rgb(255, 205, 245),
        white: Rgb(255, 255, 255),
        halo_white: Rgb(255, 255, 255),
        fg: Tone::plain(Rgb(255, 255, 255)),
        rim: Tone::plain(Rgb(220, 230, 255)),
        glass: Tone::plain(Rgb(170, 190, 255)),
        hollow: Tone::plain(Rgb(4, 5, 9)),
        tile: Glaze {
            near: Ramp::new(Tone::plain(Rgb(170, 190, 255)), 0.085, 0.05),
            far: Ramp::new(Tone::plain(Rgb(120, 140, 220)), 0.025, 0.02),
            reach: (1.0, 1.0),
        },
        tile_rim: [
            Ramp::new(Tone::plain(Rgb(220, 230, 255)), 0.26, 0.3),
            Ramp::new(Tone::plain(Rgb(220, 230, 255)), 0.06, 0.1),
            Ramp::new(Tone::plain(Rgb(220, 230, 255)), 0.10, 0.16),
        ],
        tile_shadow: None,
        chip: Glaze {
            near: Ramp::new(Tone::plain(Rgb(170, 190, 255)), 0.085, 0.05),
            far: Ramp::new(Tone::plain(Rgb(120, 140, 220)), 0.025, 0.02),
            reach: (1.0, 1.0),
        },
        chip_rim: ChipRim {
            tone: Tone::plain(Rgb(220, 230, 255)),
            alpha: [0.08, 0.3],
            profile: [1.0, 0.35, 0.55],
            width_pt: None,
        },
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
        gate_metal: [Rgb::hex(0x3a4152), Rgb::hex(0x1a1e29), Rgb::hex(0x0b0d13)],
        aperture: Rgb::hex(0x05060a),
        mirror_glass: [
            Rgb(190, 210, 255).alpha(0.3),
            Rgb(190, 210, 255).alpha(0.08),
        ],
        bender_glass: [
            Rgb(190, 170, 255).alpha(0.3),
            Rgb(190, 170, 255).alpha(0.08),
        ],
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
        lit: LitTone {
            halo: None,
            arc_width: 0.045,
            core_mid: 0.95,
            core_end: 0.0,
            core_radius: 0.225,
            yellow_mid: None,
            ring_alpha: 0.9,
            ring_width: 0.02,
            spoilt_shade: None,
        },
        text: TextTones {
            primary: Rgb::hex(0xe8edff),
            body: Rgb::hex(0xc3cbe6),
            muted: Rgb::hex(0x8a94b8),
            accent: Rgb::hex(0x8ab4ff),
        },
        panel_fill: Rgb(14, 17, 30).alpha(0.78),
        panel_rim: [
            Rgb(220, 230, 255).alpha(0.22),
            Rgb(220, 230, 255).alpha(0.05),
        ],
        panel_shadow: None,
        board_panel: None,
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
        next_shadow: None,
        ink_core: Rgb(20, 26, 46),
    };

    /// Comp B, "fully light" (spec §2 decision 8, §7 Light column).
    pub const LIGHT: Palette = Palette {
        light_mode: LightMode::Over,
        ink: Rgb::hex(0xeef1f6),
        ground: [Rgb::hex(0xf4f6fa), Rgb::hex(0xe9ecf3)],
        // White α 0.9 → 0 over 0.55 of the diagonal: the 0.45 stop is that line's value there.
        lift: [
            Rgb(255, 255, 255).alpha(0.9),
            Rgb(255, 255, 255).alpha(0.9 * (0.1 / 0.55)),
        ],
        lift_end: Rgb(255, 255, 255).alpha(0.0),
        vignette: Rgb(0, 0, 0).alpha(0.0),
        grain: 0.0,
        red: Rgb::hex(0xf0213a),
        green: Rgb::hex(0x08b85a),
        blue: Rgb::hex(0x2457ff),
        core_red: Rgb(255, 255, 255),
        core_green: Rgb(255, 255, 255),
        core_blue: Rgb(255, 255, 255),
        yellow: Rgb(242, 183, 5),
        cyan: Rgb(0, 168, 204),
        magenta: Rgb(205, 40, 210),
        core_yellow: Rgb(255, 255, 255),
        core_cyan: Rgb(255, 255, 255),
        core_magenta: Rgb(255, 255, 255),
        white: Rgb(255, 255, 255),
        halo_white: Rgb(127, 147, 207),
        fg: Tone::plain(Rgb(34, 42, 70)),
        rim: Tone {
            rgb: Rgb(27, 35, 64),
            scale: 1.3,
            add: 0.0,
        },
        glass: Tone {
            rgb: Rgb(70, 84, 130),
            scale: 0.9,
            add: 0.0,
        },
        hollow: Tone {
            rgb: Rgb(255, 255, 255),
            scale: 1.0,
            add: 0.1,
        },
        tile: Glaze {
            near: Ramp::new(Tone::plain(Rgb(255, 255, 255)), 0.7, 0.25),
            far: Ramp::new(Tone::plain(Rgb(223, 228, 242)), 0.45, 0.25),
            reach: (0.4, 1.0),
        },
        tile_rim: [Ramp::new(Tone::plain(Rgb(27, 35, 64)), 0.12, 0.1); 3],
        tile_shadow: Some(Ramp::new(Tone::plain(Rgb(27, 35, 64)), 0.08, 0.06)),
        chip: Glaze {
            near: Ramp::new(Tone::plain(Rgb(255, 255, 255)), 0.55, 0.45),
            far: Ramp::new(Tone::plain(Rgb(228, 232, 242)), 0.5, 0.4),
            reach: (0.4, 1.0),
        },
        chip_rim: ChipRim {
            tone: Tone::plain(Rgb(27, 35, 64)),
            alpha: [0.07, 0.16],
            profile: [1.0, 1.0, 1.0],
            width_pt: Some(1.0),
        },
        grid_line: Rgb(27, 35, 64),
        grid_line_alpha: [0.13, 0.10, 0.045],
        grid_dot: Rgb(27, 35, 64),
        grid_dot_alpha: [0.32, 0.12],
        wall: WallTones {
            top: Rgb::hex(0x4f586e),
            bottom: Rgb::hex(0x262b38),
            highlight: Rgb(255, 255, 255).alpha(0.5),
            shade: Rgb(0, 0, 0).alpha(0.35),
            rim: Rgb(16, 20, 34).alpha(0.7),
            hatch: Rgb(255, 255, 255).alpha(0.08),
            shadow: Rgb(27, 35, 64).alpha(0.38),
        },
        // Metal stays dark in both themes (spec §7.4); the gate has its light plate.
        metal: [Rgb::hex(0x3a4152), Rgb::hex(0x1a1e29), Rgb::hex(0x0b0d13)],
        gate_metal: [Rgb::hex(0x5a6378), Rgb::hex(0x353b4b), Rgb::hex(0x1d212c)],
        aperture: Rgb::hex(0x05060a),
        // glass(0.35) → glass(0.1) (the comps' light mirror).
        mirror_glass: [Rgb(70, 84, 130).alpha(0.315), Rgb(70, 84, 130).alpha(0.09)],
        bender_glass: [Rgb(70, 84, 130).alpha(0.315), Rgb(70, 84, 130).alpha(0.09)],
        prism_tints: [
            Rgb(170, 160, 255).alpha(0.16),
            Rgb(120, 220, 255).alpha(0.12),
        ],
        // rim(0.35), the comps' conduit rails (ruling L9).
        conduit_rail: Rgb(27, 35, 64).alpha(0.455),
        // The glass ink: ice white would vanish on a white tile (ruling L9).
        teleport_ice: Rgb(70, 84, 130),
        complement: [Rgb::hex(0x262b38), Rgb::hex(0xd8def0)],
        gate_rim: Rgb(255, 255, 255).alpha(0.35),
        gate_channel: Rgb(255, 255, 255).alpha(0.45),
        mirror_glint: Rgb(170, 190, 255).alpha(0.22),
        star_glint: Rgb(200, 215, 255),
        lit: LitTone {
            halo: Some((0.45, 0.62)),
            arc_width: 0.05,
            core_mid: 0.9,
            core_end: 0.15,
            core_radius: 0.23,
            yellow_mid: Some(Rgb(255, 230, 128)),
            ring_alpha: 1.0,
            ring_width: 0.035,
            spoilt_shade: Some((Rgb(27, 35, 64).alpha(0.12), 0.45)),
        },
        text: TextTones {
            primary: Rgb::hex(0x141a2e),
            body: Rgb::hex(0x3a4566),
            muted: Rgb::hex(0x6b7594),
            accent: Rgb::hex(0x2f6bff),
        },
        panel_fill: Rgb(255, 255, 255).alpha(0.9),
        panel_rim: [Rgb(27, 35, 64).alpha(0.10), Rgb(27, 35, 64).alpha(0.07)],
        panel_shadow: Some(Shadow {
            colour: Rgb(27, 35, 64).alpha(0.10),
            blur_pt: 18.0,
            drop_pt: 4.0,
        }),
        board_panel: Some(BoardPanel {
            fill: [Rgb::hex(0xfbfcfe), Rgb::hex(0xf1f3f8), Rgb::hex(0xe4e8f1)],
            rim: Rgb(27, 35, 64).alpha(0.12),
            shadow: Shadow {
                colour: Rgb(27, 35, 64).alpha(0.14),
                blur_pt: 28.0,
                drop_pt: 10.0,
            },
        }),
        // Derived from the light panel tokens (spec §7.1); tuned in the still review (ruling L13):
        // a panel the board does not ghost through, and a mid-tone veil, which dims the page and
        // lifts ink alike, leaving 40% of their contrast as the dark scrim does (an ink scrim
        // left near-black lettering near-black, so it read undimmed).
        sheet_fill: Rgb(255, 255, 255).alpha(0.985),
        sheet_shadow: Rgb(27, 35, 64).alpha(0.25),
        scrim: Rgb(110, 120, 152).alpha(0.6),
        pill_fill: Rgb(27, 35, 64).alpha(0.04),
        pill_rim: Rgb(27, 35, 64).alpha(0.16),
        well_fill: Rgb(222, 227, 238).alpha(0.9),
        well_shade: Rgb(27, 35, 64).alpha(0.16),
        drag_shadow: Rgb(27, 35, 64).alpha(0.35),
        chip_digit: [
            Rgb(107, 117, 148).alpha(0.45),
            Rgb(58, 69, 102).alpha(0.9),
            Rgb::hex(0x141a2e).alpha(1.0),
        ],
        padlock: Rgb(107, 117, 148).alpha(0.55),
        current_fill: Rgb(255, 255, 255).alpha(1.0),
        current_glow: Rgb(47, 107, 255).alpha(0.35),
        current_rim: Rgb::hex(0x2f6bff).alpha(1.0),
        solved_dot: Rgb::hex(0x141a2e),
        // Beam lettering's tokens: unused in light (ink lettering, ruling L6).
        letter_halo: Rgb(150, 175, 255),
        letter_glint: [Rgb(200, 220, 255), Rgb(160, 190, 255)],
        letter_core: Rgb(190, 210, 255),
        next_fill: [Rgb::hex(0x2f6bff).alpha(1.0), Rgb::hex(0x2f6bff).alpha(1.0)],
        next_rim: Rgb::hex(0x2f6bff).alpha(0.0),
        next_ink: Rgb(255, 255, 255).alpha(0.97),
        next_shadow: Some(Shadow {
            colour: Rgb(47, 107, 255).alpha(0.35),
            blur_pt: 10.0,
            drop_pt: 3.0,
        }),
        ink_core: Rgb(20, 26, 46),
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

    /// A beam mask's colour: a channel's own, a secondary's own saturated colour (yellow, cyan,
    /// magenta), white for all three. Mask 0 is black. Two beams of different masks that cross
    /// still add as light; only a beam that carries the mask is drawn in its colour.
    pub fn mix(&self, mask: Colour) -> Rgb {
        match mask & (RED | GREEN | BLUE) {
            0 => Rgb(0, 0, 0),
            m if m == RED | GREEN => self.yellow,
            m if m == GREEN | BLUE => self.cyan,
            m if m == RED | BLUE => self.magenta,
            m => self.channel(m),
        }
    }

    /// The glow colour of a mask: its mix; white light's own (dark white, light blue-grey).
    pub fn halo(&self, mask: Colour) -> Rgb {
        if mask & (RED | GREEN | BLUE) == RED | GREEN | BLUE {
            self.halo_white
        } else {
            self.mix(mask)
        }
    }

    /// A beam's core: hot, near white, tinted toward its colour; white for all three channels.
    pub fn core(&self, mask: Colour) -> Rgb {
        match mask & (RED | GREEN | BLUE) {
            RED => self.core_red,
            GREEN => self.core_green,
            BLUE => self.core_blue,
            m if m == RED | GREEN => self.core_yellow,
            m if m == GREEN | BLUE => self.core_cyan,
            m if m == RED | BLUE => self.core_magenta,
            _ => self.white,
        }
    }

    /// This palette with every colour replaced by `rgb` (alphas and rules kept): the sentinel the
    /// "colours come only from the palette" tests draw with.
    pub fn recoloured(&self, rgb: Rgb) -> Palette {
        let c = |_: Rgb| rgb;
        let a = |x: Rgba| rgb.alpha(x.a);
        let t = |x: Tone| Tone { rgb, ..x };
        let r = |x: Ramp| Ramp {
            tone: t(x.tone),
            ..x
        };
        let g = |x: Glaze| Glaze {
            near: r(x.near),
            far: r(x.far),
            ..x
        };
        let s = |x: Shadow| Shadow {
            colour: a(x.colour),
            ..x
        };
        let p = self;
        Palette {
            light_mode: p.light_mode,
            ink: c(p.ink),
            ground: [rgb; 2],
            lift: p.lift.map(a),
            lift_end: a(p.lift_end),
            vignette: a(p.vignette),
            grain: p.grain,
            red: rgb,
            green: rgb,
            blue: rgb,
            core_red: rgb,
            core_green: rgb,
            core_blue: rgb,
            yellow: rgb,
            cyan: rgb,
            magenta: rgb,
            core_yellow: rgb,
            core_cyan: rgb,
            core_magenta: rgb,
            white: rgb,
            halo_white: rgb,
            fg: t(p.fg),
            rim: t(p.rim),
            glass: t(p.glass),
            hollow: t(p.hollow),
            tile: g(p.tile),
            tile_rim: p.tile_rim.map(r),
            tile_shadow: p.tile_shadow.map(r),
            chip: g(p.chip),
            chip_rim: ChipRim {
                tone: t(p.chip_rim.tone),
                ..p.chip_rim
            },
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
            gate_metal: [rgb; 3],
            aperture: rgb,
            mirror_glass: p.mirror_glass.map(a),
            bender_glass: p.bender_glass.map(a),
            prism_tints: p.prism_tints.map(a),
            conduit_rail: a(p.conduit_rail),
            teleport_ice: rgb,
            complement: [rgb; 2],
            gate_rim: a(p.gate_rim),
            gate_channel: a(p.gate_channel),
            mirror_glint: a(p.mirror_glint),
            star_glint: rgb,
            lit: LitTone {
                yellow_mid: p.lit.yellow_mid.map(c),
                spoilt_shade: p.lit.spoilt_shade.map(|(s, r)| (a(s), r)),
                ..p.lit
            },
            text: TextTones {
                primary: rgb,
                body: rgb,
                muted: rgb,
                accent: rgb,
            },
            panel_fill: a(p.panel_fill),
            panel_rim: p.panel_rim.map(a),
            panel_shadow: p.panel_shadow.map(s),
            board_panel: p.board_panel.map(|b| BoardPanel {
                fill: [rgb; 3],
                rim: a(b.rim),
                shadow: s(b.shadow),
            }),
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
            next_shadow: p.next_shadow.map(s),
            ink_core: rgb,
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
    fn secondaries_have_their_own_saturated_colours() {
        let p = &Palette::DARK;
        assert_eq!(p.mix(RED), Rgb(255, 34, 52), "a channel is its own");
        assert_eq!(p.mix(RED | GREEN), p.yellow);
        assert_eq!(p.mix(GREEN | BLUE), p.cyan);
        assert_eq!(p.mix(RED | BLUE), p.magenta);
        assert_eq!(p.mix(WHITE), Rgb(255, 255, 255));
        assert_eq!(p.mix(0), Rgb(0, 0, 0));
        // Saturated: one component well under the others, not the channels' pale sum.
        for m in [RED | GREEN, GREEN | BLUE, RED | BLUE] {
            let Rgb(r, g, b) = p.mix(m);
            assert!(r.min(g).min(b) < 60, "{m}: {:?}", p.mix(m));
        }
        assert_eq!(
            p.halo(RED | BLUE),
            p.mix(RED | BLUE),
            "dark halos are the mix"
        );
        assert_eq!(p.core(GREEN), Rgb(210, 255, 226));
        assert_eq!(p.core(RED | GREEN), p.core_yellow);
        assert_eq!(p.core(WHITE), Rgb(255, 255, 255));
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

    #[test]
    fn the_light_palette_is_the_specs_light_column() {
        let p = &Palette::LIGHT;
        assert_eq!(Theme::Light.palette(), p);
        assert_eq!(p.light_mode, LightMode::Over);
        assert_eq!(
            (p.red, p.green, p.blue),
            (Rgb::hex(0xf0213a), Rgb::hex(0x08b85a), Rgb::hex(0x2457ff))
        );
        assert_eq!(p.mix(RED | GREEN), Rgb(242, 183, 5));
        assert_eq!(p.mix(GREEN | BLUE), Rgb(0, 168, 204));
        assert_eq!(p.mix(RED | BLUE), Rgb(205, 40, 210));
        assert_eq!(
            p.halo(WHITE),
            Rgb(127, 147, 207),
            "white's halo is blue-grey"
        );
        assert_eq!(p.halo(RED), p.mix(RED));
        assert_eq!(
            p.core(RED | GREEN),
            Rgb(255, 255, 255),
            "light cores are white"
        );
        assert_eq!(p.fg.at(0.5), Rgb(34, 42, 70).alpha(0.5));
        assert!((p.rim.at(0.5).a - 0.65).abs() < 1e-6 && (p.rim.at(0.9).a - 1.0).abs() < 1e-6);
        assert!((p.glass.at(0.5).a - 0.45).abs() < 1e-6);
        assert!((p.hollow.at(0.6).a - 0.7).abs() < 1e-6);
        assert_eq!(p.text.primary, Rgb::hex(0x141a2e));
        assert_eq!(p.text.accent, Rgb::hex(0x2f6bff));
        assert_eq!(p.scrim, Rgb(110, 120, 152).alpha(0.6));
        assert_eq!(p.ground, [Rgb::hex(0xf4f6fa), Rgb::hex(0xe9ecf3)]);
        assert_eq!(p.grain, 0.0);
        assert!(p.board_panel.is_some() && p.panel_shadow.is_some() && p.tile_shadow.is_some());
        assert_eq!(p.lit.halo, Some((0.45, 0.62)));
        assert_eq!(Theme::ALL, [Theme::Dark, Theme::Light]);
    }

    /// Ruling L1: every dark token in its new shape computes exactly what the phase-1 recipe did
    /// (the same f32 operations), so no dark pixel moves.
    #[test]
    fn dark_tokens_in_their_new_shapes_keep_their_values() {
        let p = &Palette::DARK;
        let (glass, rim) = (
            Tone::plain(Rgb(170, 190, 255)),
            Tone::plain(Rgb(220, 230, 255)),
        );
        for h in [0.0, 0.4, 1.0] {
            assert_eq!(p.tile.near.at(h), glass.at(0.085 + 0.05 * h));
            assert_eq!(p.tile.far.at(h), Rgb(120, 140, 220).alpha(0.025 + 0.02 * h));
            assert_eq!(p.tile_rim[0].at(h), rim.at(0.26 + 0.3 * h));
            assert_eq!(p.tile_rim[1].at(h), rim.at(0.06 + 0.1 * h));
            assert_eq!(p.tile_rim[2].at(h), rim.at(0.10 + 0.16 * h));
            assert_eq!(p.chip, p.tile);
        }
        assert_eq!(p.tile.reach, (1.0, 1.0));
        for a in [0.08, 0.3] {
            assert_eq!(p.chip_rim.tone.at(p.chip_rim.profile[0] * a), rim.at(a));
            assert_eq!(
                p.chip_rim.tone.at(p.chip_rim.profile[1] * a),
                rim.at(0.35 * a)
            );
            assert_eq!(
                p.chip_rim.tone.at(p.chip_rim.profile[2] * a),
                rim.at(0.55 * a)
            );
        }
        assert_eq!(p.panel_rim, [rim.at(0.22), rim.at(0.05)]);
        assert_eq!(
            p.mirror_glass,
            [
                Rgb(190, 210, 255).alpha(0.3),
                Rgb(190, 210, 255).alpha(0.08)
            ]
        );
        assert_eq!(
            p.bender_glass,
            [
                Rgb(190, 170, 255).alpha(0.3),
                Rgb(190, 170, 255).alpha(0.08)
            ]
        );
        assert_eq!(p.gate_metal, p.metal);
        assert_eq!(p.halo(WHITE), p.mix(WHITE));
        assert_eq!(p.ground, [p.ink, p.ink]);
        assert_eq!(p.lift_end, p.ink.opaque().with_alpha(0.0));
        assert_eq!(
            (p.tile_shadow, p.panel_shadow, p.board_panel, p.next_shadow),
            (None, None, None, None)
        );
        assert_eq!(p.lit.arc_width, 0.045);
        assert_eq!(p.lit.halo, None);
    }

    #[test]
    fn recolouring_reaches_every_light_colour() {
        let s = Palette::LIGHT.recoloured(Rgb(255, 0, 255));
        assert_eq!(s.halo(WHITE), Rgb(255, 0, 255));
        assert_eq!(s.ground, [Rgb(255, 0, 255); 2]);
        assert_eq!(s.tile.far.at(1.0).r, 1.0);
        assert_eq!(s.board_panel.unwrap().fill, [Rgb(255, 0, 255); 3]);
        assert_eq!(s.lit.yellow_mid, Some(Rgb(255, 0, 255)));
        assert_eq!(s.chip_rim.tone.rgb, Rgb(255, 0, 255));
        assert_eq!(s.light_mode, LightMode::Over, "rules kept");
    }

    /// Review T5 M4: Next ▶'s ink label goes with `Over` and its shadow with the token, so the
    /// two agree in every theme.
    #[test]
    fn the_next_pill_has_a_shadow_exactly_in_over() {
        for theme in Theme::ALL {
            let p = theme.palette();
            assert_eq!(
                p.next_shadow.is_some(),
                p.light_mode == LightMode::Over,
                "{theme:?}"
            );
        }
    }
}
