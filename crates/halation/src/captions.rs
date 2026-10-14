//! `captions`: the typeset strings around the board (spec §10): up to 8, each a role, lumen's
//! text, a rect in device px, a style and an alpha. The shells set them in their own fonts
//! (HTML spans on the web). Ruling 6: credits are separate captions sharing one rect (empty on a
//! compact card: the page shows them in Help); ruling 7: a lettered win header has a hidden one.

use lumen::{TextBoxes, TextRole};

use crate::DeviceRect;
use crate::lettering::{WEIGHT, letterable, level_label, width};
use crate::tableau::{CARD_PAD, HEADER_TRACK, LEVEL_CAP, PtRect, Tableau};

pub const MAX_CAPTIONS: usize = 8;

/// The space between the lettered "LEVEL n" and a compact card's level name (pt).
const NAME_GAP: f32 = 12.0;

/// How a caption is set (spec §10 table; the page maps each to a CSS class).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaptionStyle {
    /// Semibold 17 pt, primary: the level name, or a win text that cannot be lettered.
    #[default]
    Title,
    /// Semibold 15 pt, right-aligned, ellipsised: the level name on a compact header row.
    TitleCompact,
    /// Regular 15 pt (shrinking to 13 and 11), line height 1.4, body colour.
    Body,
    /// Medium 13 pt, muted, right-aligned: "(won)".
    Badge,
    /// Regular 12 pt, muted, one ellipsised line; consecutive credits are joined with " · ".
    Credit,
    /// Medium 13 pt, primary: the Levels pill's label.
    Pill,
    /// Not shown; read by screen readers (the lettered win header).
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptionRole {
    Text(TextRole),
    LevelsLabel,
}

impl CaptionRole {
    /// The number the page sees: lumen's text role's index (0–8), or 9.
    pub fn code(self) -> u8 {
        match self {
            CaptionRole::Text(role) => role as u8,
            CaptionRole::LevelsLabel => 9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caption<'a> {
    pub role: CaptionRole,
    pub text: &'a str,
    pub rect: DeviceRect,
    pub style: CaptionStyle,
    pub alpha: f32,
}

impl Default for Caption<'_> {
    fn default() -> Self {
        Caption {
            role: CaptionRole::LevelsLabel,
            text: "",
            rect: DeviceRect::default(),
            style: CaptionStyle::Hidden,
            alpha: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Captions<'a> {
    items: [Caption<'a>; MAX_CAPTIONS],
    len: usize,
}

impl<'a> Captions<'a> {
    pub fn as_slice(&self) -> &[Caption<'a>] {
        &self.items[..self.len]
    }

    /// Adds `c`; a ninth is dropped (the card never makes more than 7).
    pub fn push(&mut self, c: Caption<'a>) {
        if self.len < MAX_CAPTIONS {
            self.items[self.len] = c;
            self.len += 1;
        }
    }

    /// FNV-1a over every caption's role, style, rect, alpha and text: equal fingerprints mean the
    /// shell's typeset text is still right.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for c in self.as_slice() {
            mix(&[c.role.code(), c.style as u8]);
            for v in [c.rect.left, c.rect.top, c.rect.right, c.rect.bottom] {
                mix(&v.to_le_bytes());
            }
            mix(&c.alpha.to_bits().to_le_bytes());
            mix(c.text.as_bytes());
            mix(&[0xff]);
        }
        h
    }
}

/// `r` less 2 pt on each side horizontally (the comps' text inset inside the card's padding).
fn inset(r: PtRect) -> PtRect {
    PtRect {
        x: r.x + 2.0,
        w: (r.w - 4.0).max(0.0),
        ..r
    }
}

/// The card header's lettering of `text` at cap `cap_pt`, where the darkroom letters it: the
/// laid-out line in device px, grown by a stroke's weight on every side (its ink, not its glow).
pub fn lettered_header(tableau: &Tableau, text: &str, cap_pt: f32) -> DeviceRect {
    let cap = cap_pt * tableau.px_per_pt();
    let (x, y) = tableau.header_lettering_at(cap_pt);
    let reach = (WEIGHT * cap).ceil() as i32;
    let w = (width(text, HEADER_TRACK) * cap).ceil() as i32;
    DeviceRect::new(
        x - reach,
        y - reach,
        x + w + reach,
        y + cap.ceil() as i32 + reach,
    )
}

/// A shown caption of lumen's `role`.
fn text<'a>(role: TextRole, text: &'a str, rect: DeviceRect, style: CaptionStyle) -> Caption<'a> {
    Caption {
        role: CaptionRole::Text(role),
        text,
        rect,
        style,
        alpha: 1.0,
    }
}

/// The card's captions for lumen's text boxes of the shown state (spec §7.5, §10).
pub fn caption_card<'a>(
    tableau: &Tableau,
    texts: &TextBoxes<'a>,
    slot: usize,
    won: bool,
    next_open: bool,
) -> Captions<'a> {
    let parts = &tableau.card_parts;
    let at = |r: PtRect| tableau.device(r);
    let bottom = tableau.card.y + tableau.card.h - CARD_PAD;
    // A level without a name (lumen emits no `LevelName`) gives its name row to the hint.
    let named = texts
        .as_slice()
        .iter()
        .any(|b| b.role == TextRole::LevelName);
    let hint_box = if named {
        parts.body
    } else {
        parts.nameless_body
    };
    let mut out = Captions::default();
    for b in texts.as_slice() {
        let (role, words) = (b.role, b.text);
        let caption = match role {
            TextRole::Win if letterable(words) => {
                text(role, words, at(parts.header), CaptionStyle::Hidden)
            }
            TextRole::Win => {
                let w = (parts.badge.x - 8.0 - parts.header.x).max(0.0);
                let rect = inset(PtRect { w, ..parts.header });
                text(role, words, at(rect), CaptionStyle::Title)
            }
            // Ruling 20: lumen emits "(won)" only while the level is not won.
            TextRole::Won => text(role, words, at(parts.badge), CaptionStyle::Badge),
            TextRole::LevelName if won => continue,
            TextRole::LevelName => match parts.name {
                Some(name) => text(role, words, at(inset(name)), CaptionStyle::Title),
                // Compact: on the header row, from just right of the lettered "LEVEL n" (as the
                // darkroom places it) to just left of the badge.
                None => {
                    let mut buf = [0u8; 9];
                    let lettered = lettered_header(tableau, level_label(slot, &mut buf), LEVEL_CAP);
                    let row = at(parts.header);
                    let left = lettered.right + (NAME_GAP * tableau.px_per_pt()).round() as i32;
                    let right = tableau.easel_px_x(parts.badge.x - 4.0).round() as i32;
                    let rect = DeviceRect::new(left, row.top, right.max(left), row.bottom);
                    text(role, words, rect, CaptionStyle::TitleCompact)
                }
            },
            TextRole::Hint => text(role, words, at(inset(hint_box)), CaptionStyle::Body),
            TextRole::ClickNext => {
                let rect = if next_open {
                    parts.won_body
                } else {
                    PtRect {
                        h: (bottom - parts.won_body.y).max(0.0),
                        ..parts.won_body
                    }
                };
                text(role, words, at(inset(rect)), CaptionStyle::Body)
            }
            TextRole::Freeware | TextRole::MoreLevels | TextRole::ThankYou | TextRole::SiteUrl => {
                let rect = tableau.credits.map_or_else(DeviceRect::default, at);
                text(role, words, rect, CaptionStyle::Credit)
            }
        };
        out.push(caption);
    }
    if let Some(pill) = parts.levels {
        let label = PtRect {
            x: pill.x + 28.0,
            y: pill.y,
            w: (pill.w - 36.0).max(0.0),
            h: pill.h,
        };
        out.push(Caption {
            role: CaptionRole::LevelsLabel,
            text: "Levels",
            rect: at(label),
            style: CaptionStyle::Pill,
            alpha: 1.0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caption(text: &str) -> Caption<'_> {
        Caption {
            role: CaptionRole::Text(TextRole::Hint),
            text,
            rect: DeviceRect::new(0, 0, 10, 10),
            style: CaptionStyle::Body,
            alpha: 1.0,
        }
    }

    #[test]
    fn nine_captions_keep_eight() {
        let mut c = Captions::default();
        for _ in 0..9 {
            c.push(caption("a"));
        }
        assert_eq!(c.as_slice().len(), MAX_CAPTIONS);
    }

    #[test]
    fn the_fingerprint_follows_text_rect_and_style() {
        let mut a = Captions::default();
        a.push(caption("hint"));
        let mut b = a;
        assert_eq!(a.fingerprint(), b.fingerprint());
        b = Captions::default();
        b.push(caption("hint!"));
        assert_ne!(a.fingerprint(), b.fingerprint());
        let mut c = Captions::default();
        c.push(Caption {
            style: CaptionStyle::Badge,
            ..caption("hint")
        });
        assert_ne!(a.fingerprint(), c.fingerprint());
        let mut d = Captions::default();
        d.push(Caption {
            rect: DeviceRect::new(0, 0, 10, 11),
            ..caption("hint")
        });
        assert_ne!(a.fingerprint(), d.fingerprint());
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(CaptionRole::Text(TextRole::Win).code(), 0);
        assert_eq!(CaptionRole::Text(TextRole::SiteUrl).code(), 8);
        assert_eq!(CaptionRole::LevelsLabel.code(), 9);
        assert_eq!(CaptionStyle::Hidden as u8, 6);
    }
}
