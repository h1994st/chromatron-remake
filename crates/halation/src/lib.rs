//! Chromatron's modern look ("lightbox"): layout, vector art, additive light and motion around
//! `lumen`. Pure and platform-free like lumen: the shells hand it a pixel buffer and present it.

pub mod backdrop;
pub mod beamline;
pub mod bloom;
pub mod captions;
pub mod choreography;
pub mod darkroom;
pub mod easel;
pub mod etch;
pub mod flourish;
pub(crate) mod gouache;
pub mod lettering;
pub mod lightbox;
pub mod masonry;
pub mod patchwork;
pub mod piece_art;
pub mod playbill;
pub mod specimen;
pub mod spectrum;
pub mod tableau;
pub mod vitrine;

pub use beamline::Reach;
pub use captions::{Caption, CaptionRole, CaptionStyle, Captions};
pub use choreography::{Choreography, FRAME_MS, Moment, SETTLE_MS};
pub use darkroom::{Output, Scene};
pub use easel::{Easel, Insets};
pub use lightbox::{Lightbox, MAX_DAMAGE, Painted, Route};
pub use patchwork::Patchwork;
pub use spectrum::{
    BoardPanel, ChipRim, Glaze, LightMode, LitTone, Palette, Ramp, Rgb, Rgba, Shadow, Theme, Tone,
};
pub use tableau::{
    Arrangement, CardParts, Control, Sheet, Steps, Tableau, render_scale, stage_height,
};

/// A rectangle in device pixels, `[left, right) × [top, bottom)`.
pub type DeviceRect = lumen::Rect;

/// Empties `buffer` with room for exactly `len` items, for a reshape (spec §14 memory). It
/// allocates afresh when it must grow (`Vec`'s amortised growth would nearly double a
/// canvas-sized buffer) or when `len` is under half the capacity (a much smaller canvas gives
/// the memory back); otherwise it keeps the allocation, so a same-size or slightly smaller
/// reshape allocates nothing and a live resize doesn't churn.
#[inline(never)]
pub(crate) fn make_room<T>(buffer: &mut Vec<T>, len: usize) {
    buffer.clear();
    if len > buffer.capacity() || len < buffer.capacity() / 2 {
        let held = buffer.capacity();
        // Free the old allocation before taking the new one, so the peak stays one buffer.
        *buffer = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        let room = if held > 0 && len > held {
            len + len / GROWTH_SLACK_DIV
        } else {
            len
        };
        #[cfg(target_arch = "wasm32")]
        let room = {
            let _ = held;
            len
        };
        buffer.reserve_exact(room);
    }
}

/// Natively a buffer that must grow takes 1/`GROWTH_SLACK_DIV` more room than asked (ruling
/// G13): a macOS window dragged larger reallocates each canvas-sized buffer every ~12% of
/// growth, not on every step. The web counts every byte (its lightbox is rebuilt on resize), so
/// it reserves exactly; a first sizing (nothing held yet) reserves exactly everywhere.
#[cfg(not(target_arch = "wasm32"))]
const GROWTH_SLACK_DIV: usize = 8;

/// `buffer` reshaped to `len` copies of `value`, sized as `make_room` sizes it.
#[inline(never)]
pub(crate) fn fit_buffer<T: Clone>(buffer: &mut Vec<T>, len: usize, value: T) {
    make_room(buffer, len);
    buffer.resize(len, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling G13, exactly: a first sizing reserves what is asked; growing a held buffer takes
    /// 1/8 more; a need within the held room keeps it; one under half of it is reserved exactly.
    #[test]
    fn make_room_reserves_exactly_then_grows_with_slack() {
        let mut v: Vec<u32> = Vec::new();
        let mut room = |len: usize| {
            if v.capacity() > 0 {
                v.push(1);
            }
            make_room(&mut v, len);
            assert!(v.is_empty(), "{len}: emptied");
            v.capacity()
        };
        assert_eq!(room(100), 100, "a first sizing is exact");
        assert_eq!(room(120), 120 + 120 / 8, "grown from 100 held");
        assert_eq!(room(135), 135, "within the held room: kept");
        assert_eq!(room(80), 135, "not under half: kept");
        assert_eq!(room(60), 60, "under half: given back, exactly");
        assert_eq!(room(61), 61 + 61 / 8, "grown from 60 held");
    }
}
