//! Spec §10 and Review Focus 3: every level's captions, won and not, fit their card at every
//! worked size without overlapping each other, the lettered header or the drawn Next ▶, and the
//! lightbox reports them.

mod common;

use common::state;
use halation::captions::{CaptionRole, CaptionStyle, caption_card, lettered_header};
use halation::lettering::{letterable, level_label};
use halation::tableau::{LEVEL_CAP, WIN_CAP};
use halation::{DeviceRect, Easel, Lightbox, Output, Scene, Tableau, Theme};
use lumen::{InputEvent, TextRole, stored_solution, text_boxes};
use spectrograph::Game;

fn inside(a: DeviceRect, b: DeviceRect) -> bool {
    a.left >= b.left && a.top >= b.top && a.right <= b.right && a.bottom <= b.bottom
}

fn overlap(a: DeviceRect, b: DeviceRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// The Next ▶ pill as drawn (its float edges grown to whole px).
fn drawn_next(t: &Tableau) -> DeviceRect {
    let [l, top, r, b] = t.next_pill();
    DeviceRect::new(
        l.floor() as i32,
        top.floor() as i32,
        r.ceil() as i32,
        b.ceil() as i32,
    )
}

#[test]
fn every_levels_captions_fit_the_card() {
    let easels = [
        Easel::new(1280, 800, 1.0),
        Easel::new(800, 600, 1.0),
        Easel::new(700, 370, 1.0).with_touch(true),
        Easel::new(390, 740, 1.0).with_touch(true),
        Easel::new(390, 616, 1.0).with_touch(true),
        // Scaled, so device rounding and the corner-anchored lettering are exercised.
        Easel::new(780, 1232, 2.0).with_touch(true),
        Easel::new(2450, 1671, 1.75),
    ];
    for game in Game::ALL {
        for slot in 0..50 {
            let mut s = state(game, slot, false);
            for won in [false, true] {
                if won {
                    let Some(text) = stored_solution(common::data(game), slot) else {
                        continue;
                    };
                    s.handle(InputEvent::Paste(text));
                }
                let texts = text_boxes(&s);
                let scene = Scene::of(&s);
                let mut buf = [0u8; 9];
                let header = match (s.is_won(), letterable(scene.you_win)) {
                    (false, _) => Some((level_label(s.slot(), &mut buf), LEVEL_CAP)),
                    (true, true) => Some((scene.you_win, WIN_CAP)),
                    (true, false) => None,
                };
                for easel in &easels {
                    let t = Tableau::arrange(easel);
                    let caps = caption_card(&t, &texts, s.slot(), s.is_won(), scene.next_open());
                    let card = t.device(t.card);
                    let shown: Vec<_> = caps
                        .as_slice()
                        .iter()
                        .filter(|c| {
                            c.style != CaptionStyle::Hidden && c.rect != DeviceRect::default()
                        })
                        .collect();
                    for c in &shown {
                        let home = if c.style == CaptionStyle::Credit {
                            t.device(t.credits.unwrap())
                        } else {
                            card
                        };
                        assert!(
                            inside(c.rect, home),
                            "{game} slot {slot} won {won} {easel:?}: {:?} {:?} outside",
                            c.role,
                            c.rect
                        );
                    }
                    for (i, a) in shown.iter().enumerate() {
                        for b in &shown[i + 1..] {
                            let credits =
                                a.style == CaptionStyle::Credit && b.style == CaptionStyle::Credit;
                            assert!(
                                credits || !overlap(a.rect, b.rect),
                                "{game} {slot} {easel:?}: {:?} overlaps {:?}",
                                a.role,
                                b.role
                            );
                        }
                    }
                    // Nothing typeset sits on the light: the lettered header, or Next ▶ (its
                    // hit rect and the pill as drawn from the card's corners, ≤ 1 px apart).
                    if let Some((text, cap)) = header {
                        let lettered = lettered_header(&t, text, cap);
                        for c in &shown {
                            assert!(
                                !overlap(c.rect, lettered),
                                "{game} {slot} won {won} {easel:?}: {:?} {:?} on the header {lettered:?}",
                                c.role,
                                c.rect
                            );
                        }
                    }
                    if scene.next_open() {
                        let next = [t.device(t.card_parts.next), drawn_next(&t)];
                        for c in &shown {
                            assert!(
                                next.iter().all(|&n| !overlap(c.rect, n)),
                                "{game} {slot} {easel:?}: {:?} {:?} on Next ▶ {next:?}",
                                c.role,
                                c.rect
                            );
                        }
                    }
                    if t.compact {
                        assert!(
                            caps.as_slice()
                                .iter()
                                .filter(|c| c.style == CaptionStyle::Credit)
                                .all(|c| c.rect == DeviceRect::default()),
                            "compact: credits leave the canvas"
                        );
                    }
                    let levels = caps
                        .as_slice()
                        .iter()
                        .find(|c| c.role == CaptionRole::LevelsLabel);
                    assert_eq!(
                        levels.is_some(),
                        t.picker.is_none(),
                        "the pill's label only with a sheet picker"
                    );
                    if s.is_won() {
                        assert!(
                            caps.as_slice()
                                .iter()
                                .any(|c| c.role == CaptionRole::Text(TextRole::Win)
                                    && c.style == CaptionStyle::Hidden),
                            "ruling 7"
                        );
                        assert!(
                            caps.as_slice()
                                .iter()
                                .all(|c| c.role != CaptionRole::Text(TextRole::LevelName)),
                            "a won card drops the name"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_compact_name_starts_12_pt_right_of_the_lettering() {
    let easel = Easel::new(390, 616, 1.0).with_touch(true);
    let t = Tableau::arrange(&easel);
    assert!(t.compact, "the short phone's card is compact");
    let s = state(Game::C2, 3, false);
    let texts = text_boxes(&s);
    let caps = caption_card(&t, &texts, s.slot(), false, false);
    let name = caps
        .as_slice()
        .iter()
        .find(|c| c.role == CaptionRole::Text(TextRole::LevelName))
        .expect("C2 names its levels");
    assert_eq!(name.style, CaptionStyle::TitleCompact);
    let mut buf = [0u8; 9];
    let lettered = lettered_header(&t, level_label(3, &mut buf), LEVEL_CAP);
    assert_eq!(
        name.rect.left,
        lettered.right + 12,
        "a 12-pt gap at 1 px/pt"
    );
    assert!(name.rect.right > name.rect.left, "room for the name");
}

/// A level without a name (C1's) has no name row: its hint starts right under the header, as on
/// a compact card, while a named level's hint stays under its name.
#[test]
fn a_nameless_levels_hint_takes_the_name_row() {
    let easel = Easel::new(780, 1480, 2.0).with_touch(true);
    let t = Tableau::arrange(&easel);
    let name = t.card_parts.name.expect("the portrait card has a name row");
    let hint = |game: Game| {
        let s = state(game, 0, false);
        let texts = text_boxes(&s);
        let caps = caption_card(&t, &texts, 0, false, false);
        let named = caps
            .as_slice()
            .iter()
            .any(|c| c.role == CaptionRole::Text(TextRole::LevelName));
        let hint = caps
            .as_slice()
            .iter()
            .find(|c| c.role == CaptionRole::Text(TextRole::Hint))
            .expect("a first level has a hint")
            .rect;
        (named, hint)
    };
    let header_bottom = t.device(t.card_parts.header).bottom;
    let card_bottom = t.device(t.card).bottom - (12.0 * t.px_per_pt()).round() as i32;
    let (named, nameless) = hint(Game::C1);
    assert!(!named, "C1 names no level");
    assert_eq!(nameless.top, header_bottom, "the hint follows the header");
    assert_eq!(nameless.bottom, card_bottom, "down to the bottom padding");
    let (named, hint) = hint(Game::C2);
    assert!(named, "C2 names its levels");
    assert_eq!(hint.top, t.device(name).bottom, "the hint follows the name");
    assert_eq!(hint.bottom, card_bottom);
}

#[test]
fn the_lightbox_reports_captions_only_when_they_change() {
    let mut s = state(Game::C2, 3, false);
    let easel = Easel::new(1280, 800, 1.0);
    let mut lightbox = Lightbox::new(easel, Theme::Dark, false);
    let mut out = vec![0u32; 1280 * 800];
    let first = lightbox.paint(&s, 0.0, Output::Xrgb(&mut out));
    assert!(first.captions_changed);
    let hint = first
        .captions
        .as_slice()
        .iter()
        .find(|c| c.role == CaptionRole::Text(TextRole::Hint))
        .map(|c| c.text.to_owned());
    assert_eq!(hint.as_deref(), Some(s.hint()));
    assert!(
        !lightbox
            .paint(&s, 16.0, Output::Xrgb(&mut out))
            .captions_changed,
        "nothing changed"
    );
    s.handle(InputEvent::Key(b'-'));
    assert!(
        lightbox
            .paint(&s, 32.0, Output::Xrgb(&mut out))
            .captions_changed,
        "another level"
    );
    lightbox.reset();
    assert!(
        lightbox
            .paint(&s, 48.0, Output::Xrgb(&mut out))
            .captions_changed,
        "a cold paint always reports"
    );
    let sheetless = Scene::of(&s);
    let bare = lightbox.paint_scene(sheetless, 64.0, Output::Xrgb(&mut out));
    assert!(bare.captions.as_slice().is_empty(), "a bare scene has none");
    assert!(bare.captions_changed, "paint → paint_scene drops them");
    assert!(
        lightbox
            .paint(&s, 80.0, Output::Xrgb(&mut out))
            .captions_changed,
        "paint_scene → paint brings them back"
    );
    let other = state(Game::C3, 3, false);
    assert!(
        lightbox
            .paint(&other, 96.0, Output::Xrgb(&mut out))
            .captions_changed,
        "another game"
    );
}
