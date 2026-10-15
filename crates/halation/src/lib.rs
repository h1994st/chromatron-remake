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
        // Free the old allocation before taking the new one, so the peak stays one buffer.
        *buffer = Vec::new();
        buffer.reserve_exact(len);
    }
}

/// `buffer` reshaped to `len` copies of `value`, sized as `make_room` sizes it.
#[inline(never)]
pub(crate) fn fit_buffer<T: Clone>(buffer: &mut Vec<T>, len: usize, value: T) {
    make_room(buffer, len);
    buffer.resize(len, value);
}
