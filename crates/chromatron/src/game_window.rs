//! The macOS shell: one window, the menu bar, and the event loop that feeds `lumen`.

use std::env;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use lumen::layout::STRIP_SLOTS;
use lumen::{FRAME_HEIGHT, FRAME_WIDTH, GameState, InputEvent, Rect};
use muda::MenuEvent;
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::platform::macos::{
    ActiveEventLoopExtMacOS, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS,
};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowButtons, WindowId};

use crate::AppSpec;
use crate::blit::{LETTERBOX, blit_scaled, fill_letterbox};
use crate::damage::Damage;
use crate::frame_timing::{FrameTimes, Profiler};
use crate::menus::{MenuBar, MenuCommand};
use crate::pasteboard;
use crate::pointer_keys::{Button, ButtonLatch, Held, key_byte, typed_text};
use crate::progress_file::ProgressFile;
use crate::snapshot;
use crate::typeset::TypesetCache;
use crate::user_defaults::Preferences;
use crate::viewport::{DeviceRect, Viewport, window_size};
use crate::wheel::{ScrollDelta, WheelGate};

/// The whole frame, as a damage rect.
const FULL_FRAME: Rect = Rect::new(0, 0, FRAME_WIDTH as i32, FRAME_HEIGHT as i32);

/// Events sent to the loop from outside winit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserEvent {
    Menu(MenuCommand),
}

/// Runs the game until the player quits.
pub fn run(spec: &'static AppSpec) -> ExitCode {
    let launched = Instant::now();
    let profiling = env::var_os("CHROMATRON_PROFILE").is_some_and(|v| v == "1");
    let mut logger = env_logger::Builder::new();
    logger.filter_level(log::LevelFilter::Warn);
    if profiling {
        logger.filter_module(crate::frame_timing::TARGET, log::LevelFilter::Info);
    }
    logger.parse_default_env().init();

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    builder.with_default_menu(false);
    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            log::error!("cannot start the event loop: {e}");
            return ExitCode::FAILURE;
        }
    };
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(command) = MenuCommand::from_id(event.id.as_ref()) {
            // Fails only once the loop has exited, when nothing is left to do.
            let _ = proxy.send_event(UserEvent::Menu(command));
        }
    }));

    let progress = ProgressFile::for_app(spec.support_dir, lumen::RuleSet::of(spec.data.game));
    if progress.is_none() {
        log::warn!("no home folder: progress will not be saved");
    }
    let snapshot = env::var_os("CHROMATRON_SNAPSHOT").map(PathBuf::from);
    // Snapshot mode never reads or writes the player's progress.
    let save = match (&progress, &snapshot) {
        (Some(file), None) => file.load(),
        _ => lumen::SaveData::fresh(lumen::RuleSet::of(spec.data.game)),
    };
    let mut app = App::new(
        spec,
        save,
        progress,
        snapshot,
        Profiler::new(profiling, launched),
    );
    if app.snapshot.is_some() {
        // The first paint is still a full repaint: `damage` starts as `Full` and only the
        // state changes here.
        let scene = snapshot::Scene::from_env().and_then(|scene| scene.apply(&mut app.state));
        if let Err(e) = scene {
            log::error!("snapshot: {e}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop failed: {e}");
        return ExitCode::FAILURE;
    }
    app.exit_code
}

struct App {
    spec: &'static AppSpec,
    state: GameState<'static>,
    progress: Option<ProgressFile>,
    prefs: Preferences,
    wheel_on: bool,
    menus: Option<MenuBar>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    /// The 640 × 480 frame `lumen::render` draws into.
    frame: Vec<u32>,
    /// The window's pixels: the upscaled frame, letterbox and text. Kept between presents so a
    /// partial repaint only touches its rects.
    canvas: Vec<u32>,
    viewport: Viewport,
    typeset: TypesetCache,
    damage: Damage,
    latch: ButtonLatch,
    buttons_held: u32,
    /// The pointer in frame pixels.
    cursor: (i32, i32),
    modifiers: ModifiersState,
    wheel: WheelGate,
    /// Time in `GameState::handle` since the last present.
    handle_time: Duration,
    profiler: Profiler,
    snapshot: Option<PathBuf>,
    snapshot_written: bool,
    exit_code: ExitCode,
    /// The (fullscreen, scale) the View menu currently shows.
    shown_view: Option<(bool, usize)>,
}

/// The View-menu state to show, if it differs from what is shown.
fn view_update(shown: Option<(bool, usize)>, now: (bool, usize)) -> Option<(bool, usize)> {
    (shown != Some(now)).then_some(now)
}

impl App {
    fn new(
        spec: &'static AppSpec,
        save: lumen::SaveData,
        progress: Option<ProgressFile>,
        snapshot: Option<PathBuf>,
        profiler: Profiler,
    ) -> App {
        let prefs = Preferences::open(spec.bundle_id);
        let mut state = GameState::new(spec.data, save);
        // Deliberate modern touch (the originals always opened on slot 0). Snapshots ignore it.
        if snapshot.is_none() {
            let stored = prefs.last_level_slot().and_then(|s| i64::try_from(s).ok());
            if let Some(slot) = resume_slot(stored, STRIP_SLOTS, |s| state.is_unlocked(s)) {
                state.goto_slot(slot);
            }
        }
        App {
            spec,
            state,
            progress,
            wheel_on: prefs.wheel_changes_level(),
            prefs,
            menus: None,
            window: None,
            surface: None,
            frame: vec![0; FRAME_WIDTH * FRAME_HEIGHT],
            canvas: Vec::new(),
            viewport: Viewport::fit(FRAME_WIDTH, FRAME_HEIGHT),
            typeset: TypesetCache::default(),
            damage: Damage::Full,
            latch: ButtonLatch::default(),
            buttons_held: 0,
            cursor: (-1, -1),
            modifiers: ModifiersState::empty(),
            wheel: WheelGate::default(),
            handle_time: Duration::ZERO,
            profiler,
            snapshot,
            snapshot_written: false,
            exit_code: ExitCode::SUCCESS,
            shown_view: None,
        }
    }

    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        self.menus = Some(MenuBar::install(self.wheel_on));
        let attributes = Window::default_attributes()
            .with_title(self.spec.data.ui.window_title.as_ref())
            .with_inner_size(LogicalSize::new(FRAME_WIDTH as f64, FRAME_HEIGHT as f64))
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
            .with_accepts_first_mouse(false);
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("cannot open the window: {e}"))?,
        );
        let context = Context::new(window.clone())
            .map_err(|e| format!("cannot draw into the window: {e}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| format!("cannot draw into the window: {e}"))?;
        self.surface = Some(surface);
        let size = window.inner_size();
        self.window = Some(window);
        self.resized(size);
        Ok(())
    }

    /// The window's content changed size (open, scale, fullscreen, another display).
    fn resized(&mut self, size: PhysicalSize<u32>) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return;
        };
        let width = NonZeroU32::new(size.width).unwrap_or(NonZeroU32::MIN);
        let height = NonZeroU32::new(size.height).unwrap_or(NonZeroU32::MIN);
        if let Err(e) = surface.resize(width, height) {
            log::error!("cannot resize the surface: {e}");
            return;
        }
        self.viewport = Viewport::fit(width.get() as usize, height.get() as usize);
        self.canvas
            .resize(self.viewport.width * self.viewport.height, LETTERBOX);
        self.damage.add(lumen::Redraw::Full);
        window.request_redraw();
        self.sync_view();
    }

    /// Shows the real fullscreen state and scale in the View menu when they changed.
    fn sync_view(&mut self) {
        let (Some(window), Some(menus)) = (&self.window, &self.menus) else {
            return;
        };
        let now = (window.fullscreen().is_some(), self.viewport.scale);
        if let Some((fullscreen, scale)) = view_update(self.shown_view, now) {
            menus.show_view(fullscreen, scale);
            self.shown_view = Some(now);
        }
    }

    /// Hands one event to the game and carries out what it asks for.
    fn send(&mut self, event: InputEvent) {
        let started = Instant::now();
        let slot = self.state.slot();
        let effects = self.state.handle(event);
        self.handle_time += started.elapsed();
        if self.state.slot() != slot {
            self.remember_slot();
        }
        self.damage.add(effects.redraw);
        if effects.write_save {
            self.write_save();
        }
        if let Some(text) = effects.clipboard {
            pasteboard::copy_text(&text);
        }
        if !self.damage.is_none()
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }

    /// Stores the current level for the next launch (never in snapshot mode).
    fn remember_slot(&self) {
        if self.snapshot.is_none() {
            self.prefs.set_last_level_slot(self.state.slot());
        }
    }

    fn write_save(&self) {
        if self.snapshot.is_some() {
            return;
        }
        if let Some(file) = &self.progress
            && let Err(e) = file.store(&self.state.save_bytes())
        {
            log::error!("cannot write {}: {e}", file.path().display());
        }
    }

    /// Brings the canvas up to date with the pending damage and presents it.
    fn paint(&mut self, event_loop: &ActiveEventLoop) {
        let Some(surface) = &mut self.surface else {
            return;
        };
        let damage = self.damage.take();
        let mut times = FrameTimes {
            trace: std::mem::take(&mut self.handle_time),
            ..FrameTimes::default()
        };
        // No damage means the system asked for a redraw (an expose); the canvas is current.
        if !damage.is_none() {
            let started = Instant::now();
            let boxes = lumen::render(&self.state, &mut self.frame);
            times.render = started.elapsed();
            let started = Instant::now();
            let vp = self.viewport;
            if damage == Damage::Full {
                fill_letterbox(&mut self.canvas, &vp);
                blit_scaled(&self.frame, &mut self.canvas, &vp, FULL_FRAME);
                let everything = DeviceRect {
                    left: 0,
                    top: 0,
                    right: vp.width,
                    bottom: vp.height,
                };
                self.typeset
                    .draw(boxes.as_slice(), &mut self.canvas, &vp, everything);
            } else {
                for &rect in damage.rects() {
                    blit_scaled(&self.frame, &mut self.canvas, &vp, rect);
                    if let Some(clip) = vp.device_rect(rect) {
                        self.typeset
                            .draw(boxes.as_slice(), &mut self.canvas, &vp, clip);
                    }
                }
            }
            times.blit = started.elapsed();
        }
        let started = Instant::now();
        let mut presented = false;
        // Accepted deviation from spec §6: softbuffer 0.4.8's CoreGraphics backend allocates and
        // zeroes a device-size buffer on every present. Kept until measured; if a fullscreen
        // present exceeds ~8 ms, present through our own CALayer contents instead.
        match surface.buffer_mut() {
            Ok(mut buffer) => {
                buffer.copy_from_slice(&self.canvas);
                match buffer.present() {
                    Ok(()) => presented = true,
                    Err(e) => log::error!("cannot present: {e}"),
                }
            }
            Err(e) => log::error!("cannot get the window buffer: {e}"),
        }
        times.present = started.elapsed();
        self.profiler.frame(times, damage);

        if self.snapshot.is_some() && !presented {
            self.exit_code = ExitCode::FAILURE;
            event_loop.exit();
            return;
        }
        if let Some(path) = &self.snapshot
            && !self.snapshot_written
        {
            self.snapshot_written = true;
            let vp = self.viewport;
            if let Err(e) = snapshot::write_png(path, &self.canvas, vp.width, vp.height) {
                log::error!("cannot write {}: {e}", path.display());
                self.exit_code = ExitCode::FAILURE;
            }
            event_loop.exit();
        }
    }

    fn menu(&mut self, event_loop: &ActiveEventLoop, command: MenuCommand) {
        match command {
            MenuCommand::ResetLevel => self.send(InputEvent::Key(b'r')),
            MenuCommand::NextLevel => self.send(InputEvent::Key(b'=')),
            MenuCommand::PreviousLevel => self.send(InputEvent::Key(b'-')),
            MenuCommand::ToggleWheel => {
                self.wheel_on = !self.wheel_on;
                self.prefs.set_wheel_changes_level(self.wheel_on);
                if let Some(menus) = &self.menus {
                    menus.set_wheel_checked(self.wheel_on);
                }
            }
            MenuCommand::Close => event_loop.exit(),
            MenuCommand::CopyLevel => self.send(InputEvent::Copy),
            MenuCommand::PasteLevel => {
                if let Some(text) = pasteboard::paste_text() {
                    self.send(InputEvent::Paste(text));
                }
            }
            MenuCommand::ToggleFullscreen => {
                if let Some(window) = &self.window {
                    let next = match window.fullscreen() {
                        Some(_) => None,
                        None => Some(Fullscreen::Borderless(None)),
                    };
                    window.set_fullscreen(next);
                }
            }
            MenuCommand::Scale(scale) => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                if window.fullscreen().is_none() {
                    let (width, height) = window_size(scale);
                    if let Some(size) = window.request_inner_size(PhysicalSize::new(width, height))
                    {
                        self.resized(size);
                    }
                }
                // muda flips a check item when it is clicked; show the real state again.
                self.shown_view = None;
                self.sync_view();
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Nothing animates (platform.md P9): sleep until the next event.
        event_loop.set_control_flow(ControlFlow::Wait);
        // No window tabs, so AppKit adds no tab items (or a second full-screen item) to the menus.
        event_loop.set_allows_automatic_window_tabbing(false);
        if let Err(message) = self.open_window(event_loop) {
            log::error!("{message}");
            self.exit_code = ExitCode::FAILURE;
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resized(size),
            // A button released while another app had the focus never reaches us: end the drag
            // (lumen ends it on any mouse-up; the piece stays put) before forgetting the buttons.
            WindowEvent::Focused(false) => {
                if self.buttons_held > 0 {
                    self.send(InputEvent::MouseUp {
                        x: -1,
                        y: -1,
                        alt: false,
                    });
                }
                self.buttons_held = 0;
            }
            WindowEvent::RedrawRequested => self.paint(event_loop),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.viewport.to_frame(position.x, position.y);
                // Deliberate deviation: the original dropped drags outside the content area
                // (platform.md P4). We keep forwarding them and lumen clips; harmless.
                if self.buttons_held > 0 {
                    let (x, y) = self.cursor;
                    self.send(InputEvent::MouseDragged { x, y });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Right => Button::Secondary,
                    _ => Button::Other,
                };
                let (x, y) = self.cursor;
                match state {
                    ElementState::Pressed => {
                        self.buttons_held += 1;
                        let held = Held {
                            shift: self.modifiers.shift_key(),
                            option: self.modifiers.alt_key(),
                            control: self.modifiers.control_key(),
                        };
                        let alt = self.latch.press(button, held);
                        self.send(InputEvent::MouseDown { x, y, alt });
                    }
                    ElementState::Released => {
                        // A release without a press is the window-activating click's mouse-up
                        // (its mouse-down never reaches us): drop it, or it would rotate a piece.
                        let Some(held) = self.buttons_held.checked_sub(1) else {
                            return;
                        };
                        self.buttons_held = held;
                        let alt = self.latch.release(button);
                        self.send(InputEvent::MouseUp { x, y, alt });
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.wheel_on {
                    return;
                }
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines {
                        x: f64::from(x),
                        y: f64::from(y),
                    },
                    MouseScrollDelta::PixelDelta(position) => {
                        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                        let logical = position.to_logical::<f64>(scale);
                        ScrollDelta::Pixels {
                            x: logical.x,
                            y: logical.y,
                        }
                    }
                };
                if let Some(delta) = self.wheel.feed(scroll, Instant::now()) {
                    self.send(InputEvent::Wheel { delta });
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state == ElementState::Pressed => {
                let typed = typed_text(
                    event.text.as_deref(),
                    event.text_with_all_modifiers(),
                    self.state.rules().cheat_key,
                );
                if let Some(byte) = key_byte(typed, self.modifiers.super_key(), event.repeat) {
                    self.send(InputEvent::Key(byte));
                }
            }
            _ => {}
        }
    }

    /// Fullscreen state clears only after the exit animation, with no `Resized` after it.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.sync_view();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Menu(command) = event;
        self.menu(event_loop, command);
    }

    /// Every way out (Quit, Close, the close box, logout) ends here: save as the original did.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let effects = self.state.handle(InputEvent::Quit);
        if effects.write_save {
            self.write_save();
        }
        self.remember_slot();
    }
}

/// The slot to reopen on: the stored one if it is on the strip, not slot 0 (where the game
/// starts anyway) and unlocked.
fn resume_slot(
    stored: Option<i64>,
    strip_slots: usize,
    unlocked: impl Fn(usize) -> bool,
) -> Option<usize> {
    let slot = usize::try_from(stored?).ok()?;
    (slot != 0 && slot < strip_slots && unlocked(slot)).then_some(slot)
}

#[cfg(test)]
mod tests {
    use super::{resume_slot, view_update};

    #[test]
    fn resume_needs_a_stored_unlocked_slot_other_than_the_first() {
        let open = |slot: usize| slot <= 7;
        assert_eq!(resume_slot(None, 50, open), None);
        assert_eq!(resume_slot(Some(0), 50, open), None);
        assert_eq!(resume_slot(Some(7), 50, open), Some(7));
        assert_eq!(resume_slot(Some(8), 50, open), None);
        assert_eq!(resume_slot(Some(-3), 50, open), None);
        assert_eq!(resume_slot(Some(50), 50, |_| true), None);
        assert_eq!(resume_slot(Some(49), 50, |_| true), Some(49));
        // Range is checked before `unlocked`, which indexes the strip.
        assert_eq!(
            resume_slot(Some(60), 50, |s| {
                assert!(s < 50, "unlocked called for {s}");
                true
            }),
            None
        );
    }

    #[test]
    fn the_view_menu_is_updated_only_when_its_state_changed() {
        assert_eq!(view_update(None, (false, 1)), Some((false, 1)));
        assert_eq!(view_update(Some((true, 2)), (true, 2)), None);
        assert_eq!(view_update(Some((true, 2)), (false, 2)), Some((false, 2)));
        assert_eq!(view_update(Some((false, 1)), (false, 2)), Some((false, 2)));
    }
}
