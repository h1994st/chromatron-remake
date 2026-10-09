//! Input rules both shells follow, with no platform types: mouse buttons and typed keys as the
//! original read them (platform.md P4, logic.md L7), and modern scroll events turned into the
//! original's one-level-per-event wheel.
//!
//! The wheel: a notched mouse wheel sends one event per notch, and each one steps a level, as on
//! Mac OS X 10.3. A trackpad or Magic Mouse sends a stream of small pixel deltas plus momentum
//! events; the whole stream (one gesture) steps once, after it has travelled `PIXELS_PER_STEP`. A
//! gesture ends after `QUIET_MS` without scroll events. The delta's sign is winit's (positive: the
//! content moves down), so the user's natural-scrolling setting is honoured: scrolling content
//! down (delta > 0) goes to the previous level. This deliberately differs from Mac OS X 10.3,
//! which had no natural scrolling and where rolling the wheel away from the user meant the
//! previous level. Time is milliseconds from any fixed origin, because `std::time::Instant`
//! panics on wasm32-unknown-unknown.

/// A mouse button as the original's handler distinguishes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Button 1 (left).
    Primary,
    /// Button 2 (right).
    Secondary,
    /// Middle and extra buttons. The original treats them as primary and never checks modifiers.
    Other,
}

/// Modifier keys held when a button goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub shift: bool,
    pub option: bool,
    pub control: bool,
}

/// `mouse_alt_button`: whether a gesture is the alternate (rotate clockwise) kind. Latched on a
/// primary press from Shift, Option or Control (Ctrl-click is a right click), and reused for the
/// matching release.
#[derive(Debug, Default)]
pub struct ButtonLatch {
    alt: bool,
}

impl ButtonLatch {
    /// `alt` for a button going down.
    pub fn press(&mut self, button: Button, held: Held) -> bool {
        match button {
            Button::Primary => {
                self.alt = held.shift || held.option || held.control;
                self.alt
            }
            Button::Secondary => true,
            Button::Other => false,
        }
    }

    /// `alt` for a button going up.
    pub fn release(&self, button: Button) -> bool {
        match button {
            Button::Primary => self.alt,
            Button::Secondary => true,
            Button::Other => false,
        }
    }
}

/// The byte `handle_key` gets for a key press, if any: the typed character when it is one ASCII
/// byte, with Command up (Command keys belong to the menus) and not a key repeat (the original
/// doesn't register repeats).
pub fn key_byte(text: Option<&str>, command: bool, repeat: bool) -> Option<u8> {
    if command || repeat {
        return None;
    }
    match text?.as_bytes() {
        [byte] if byte.is_ascii() => Some(*byte),
        _ => None,
    }
}

/// What a key press types for `handle_key`, given `text` (the character with Control stripped:
/// Control-O types "o") and `mac_chars` (the character with every modifier applied, NSEvent's
/// `characters`: Control-O is "\u{f}", the Mac char code the original read). The Mac char code is
/// used only when it is the game's cheat key and that key is a control character (Chromatron 4's
/// Control-O, platform.md P4), so every other key keeps the meaning it has in every game.
pub fn typed_text<'a>(
    text: Option<&'a str>,
    mac_chars: Option<&'a str>,
    cheat_key: u8,
) -> Option<&'a str> {
    match mac_chars.map(str::as_bytes) {
        Some([byte]) if *byte == cheat_key && byte.is_ascii_control() => mac_chars,
        _ => text,
    }
}

/// Logical pixels a gesture must travel before it steps.
pub const PIXELS_PER_STEP: f64 = 20.0;
/// A gap this long (milliseconds) between pixel deltas starts a new gesture.
pub const QUIET_MS: f64 = 150.0;

/// One scroll event: notched lines, or precise logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    Lines { x: f64, y: f64 },
    Pixels { x: f64, y: f64 },
}

#[derive(Debug, Default)]
pub struct WheelGate {
    last_ms: Option<f64>,
    travel: f64,
    stepped: bool,
}

impl WheelGate {
    /// The `InputEvent::Wheel` delta to send for an event at `at_ms`, if any: `1` (previous level)
    /// or `-1` (next level). The axis isn't read by the original, so the larger one counts.
    pub fn feed(&mut self, delta: ScrollDelta, at_ms: f64) -> Option<i32> {
        match delta {
            ScrollDelta::Lines { x, y } => {
                self.last_ms = None;
                sign(dominant(x, y))
            }
            ScrollDelta::Pixels { x, y } => {
                // A NaN gap (a broken clock) starts a new gesture instead of never ending one.
                let new_gesture = self.last_ms.is_none_or(|last| {
                    let gap = at_ms - last;
                    gap >= QUIET_MS || gap.is_nan()
                });
                if new_gesture {
                    self.travel = 0.0;
                    self.stepped = false;
                }
                self.last_ms = Some(at_ms);
                if self.stepped {
                    return None;
                }
                self.travel += dominant(x, y);
                if self.travel.abs() < PIXELS_PER_STEP {
                    return None;
                }
                self.stepped = true;
                sign(self.travel)
            }
        }
    }
}

fn dominant(x: f64, y: f64) -> f64 {
    if y.abs() >= x.abs() { y } else { x }
}

fn sign(v: f64) -> Option<i32> {
    if v > 0.0 {
        Some(1)
    } else if v < 0.0 {
        Some(-1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Held = Held {
        shift: false,
        option: false,
        control: false,
    };

    // The button and key tests below moved unchanged from the macOS shell.

    #[test]
    fn a_plain_left_click_is_primary_and_a_right_click_is_alternate() {
        let mut latch = ButtonLatch::default();
        assert!(!latch.press(Button::Primary, NONE));
        assert!(!latch.release(Button::Primary));
        assert!(latch.press(Button::Secondary, NONE));
        assert!(latch.release(Button::Secondary));
    }

    #[test]
    fn shift_option_or_control_click_is_alternate_through_the_release() {
        for held in [
            Held {
                shift: true,
                ..NONE
            },
            Held {
                option: true,
                ..NONE
            },
            Held {
                control: true,
                ..NONE
            },
        ] {
            let mut latch = ButtonLatch::default();
            assert!(latch.press(Button::Primary, held), "{held:?}");
            // The modifier may be let go before the release; the latch keeps the gesture's kind.
            assert!(latch.release(Button::Primary), "{held:?}");
        }
    }

    #[test]
    fn other_buttons_are_primary_whatever_is_held() {
        let mut latch = ButtonLatch::default();
        let held = Held {
            shift: true,
            ..NONE
        };
        assert!(!latch.press(Button::Other, held));
        assert!(!latch.release(Button::Other));
    }

    #[test]
    fn a_right_press_does_not_change_the_primary_latch() {
        let mut latch = ButtonLatch::default();
        latch.press(
            Button::Primary,
            Held {
                control: true,
                ..NONE
            },
        );
        latch.press(Button::Secondary, NONE);
        assert!(latch.release(Button::Primary));
    }

    #[test]
    fn keys_are_single_ascii_characters_without_command_or_repeat() {
        assert_eq!(key_byte(Some(" "), false, false), Some(b' '));
        assert_eq!(key_byte(Some("L"), false, false), Some(b'L'));
        assert_eq!(key_byte(Some("+"), false, false), Some(b'+'));
        assert_eq!(key_byte(Some("\u{1b}"), false, false), Some(0x1b));
        assert_eq!(key_byte(Some("r"), true, false), None);
        assert_eq!(key_byte(Some("r"), false, true), None);
        assert_eq!(key_byte(Some("é"), false, false), None);
        assert_eq!(key_byte(Some("ab"), false, false), None);
        assert_eq!(key_byte(None, false, false), None);
    }

    #[test]
    fn control_o_types_0x0f_only_where_it_is_the_cheat() {
        let control_o = (Some("o"), Some("\u{f}"));
        assert_eq!(
            typed_text(control_o.0, control_o.1, 0x0f),
            Some("\u{f}"),
            "Chromatron 4"
        );
        for cheat in *b"Lz^" {
            assert_eq!(typed_text(control_o.0, control_o.1, cheat), Some("o"));
        }
        assert_eq!(
            typed_text(Some("r"), Some("\u{12}"), 0x0f),
            Some("r"),
            "Control-R"
        );
        assert_eq!(typed_text(Some("^"), Some("^"), b'^'), Some("^"));
        assert_eq!(typed_text(None, None, 0x0f), None);
        let byte = key_byte(typed_text(control_o.0, control_o.1, 0x0f), false, false);
        assert_eq!(byte, Some(0x0f));
        assert_eq!(
            key_byte(typed_text(control_o.0, control_o.1, 0x0f), true, false),
            None,
            "Command-Control-O is a menu key"
        );
        assert_eq!(
            key_byte(typed_text(control_o.0, control_o.1, 0x0f), false, true),
            None,
            "repeats are ignored"
        );
    }

    // The wheel tests moved from the macOS shell, with the clock in milliseconds.

    #[test]
    fn every_wheel_notch_steps_once() {
        let mut gate = WheelGate::default();
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: 1.0 }, 0.0),
            Some(1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: 3.0 }, 5.0),
            Some(1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: 0.0, y: -1.0 }, 9.0),
            Some(-1)
        );
        assert_eq!(gate.feed(ScrollDelta::Lines { x: 0.0, y: 0.0 }, 12.0), None);
    }

    #[test]
    fn a_trackpad_gesture_with_momentum_steps_once() {
        let mut gate = WheelGate::default();
        let steps: Vec<i32> = (0..120)
            .filter_map(|i| gate.feed(ScrollDelta::Pixels { x: 0.0, y: -6.0 }, 8.0 * f64::from(i)))
            .collect();
        assert_eq!(steps, [-1]);
    }

    #[test]
    fn a_pause_starts_a_new_gesture() {
        let mut gate = WheelGate::default();
        let t = 1000.0;
        let pixels = ScrollDelta::Pixels { x: 0.0, y: 25.0 };
        assert_eq!(gate.feed(pixels, t), Some(1));
        assert_eq!(gate.feed(pixels, t + 100.0), None);
        assert_eq!(gate.feed(pixels, t + 100.0 + QUIET_MS), Some(1));
    }

    #[test]
    fn a_small_nudge_does_nothing() {
        let mut gate = WheelGate::default();
        for i in 0..3 {
            assert_eq!(
                gate.feed(ScrollDelta::Pixels { x: 0.0, y: 5.0 }, 10.0 * f64::from(i)),
                None
            );
        }
        assert_eq!(
            gate.feed(ScrollDelta::Pixels { x: 0.0, y: 5.0 }, 400.0),
            None
        );
    }

    #[test]
    fn the_larger_axis_decides() {
        let mut gate = WheelGate::default();
        assert_eq!(
            gate.feed(ScrollDelta::Lines { x: -2.0, y: 1.0 }, 0.0),
            Some(-1)
        );
        assert_eq!(
            gate.feed(ScrollDelta::Pixels { x: 30.0, y: -4.0 }, 1.0),
            Some(1)
        );
    }

    #[test]
    fn a_clock_that_is_not_a_number_starts_a_new_gesture() {
        let mut gate = WheelGate::default();
        let pixels = ScrollDelta::Pixels { x: 0.0, y: 25.0 };
        assert_eq!(gate.feed(pixels, f64::NAN), Some(1));
        assert_eq!(
            gate.feed(pixels, f64::NAN),
            Some(1),
            "never stuck in one gesture"
        );
        assert_eq!(gate.feed(pixels, 5.0), Some(1));
        assert_eq!(gate.feed(pixels, 10.0), None, "a real clock groups again");
    }
}
