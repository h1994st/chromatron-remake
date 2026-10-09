//! Chromatron in the browser: the `lumen` engine and all four games' data behind a
//! `wasm-bindgen` session that `web/main.js` drives. The helpers are plain Rust with native
//! tests; only `session` and `stage::display_scale` are exported to JavaScript.

pub mod browser_keys;
pub mod embedded_games;
pub mod pixels;
pub mod save_text;
pub mod session;
pub mod stage;

pub use session::{Flag, LevelState, Session};
pub use stage::display_scale;
