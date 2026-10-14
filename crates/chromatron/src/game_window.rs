//! The macOS shell: one window, the menu bar, and the event loop that feeds `lumen`.

use std::env;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use halation::{Control, Theme};
use lumen::layout::STRIP_SLOTS;
use lumen::{
    Button, ButtonLatch, Damage, FRAME_HEIGHT, FRAME_WIDTH, GameState, Held, InputEvent, Rect,
    ScrollDelta, WheelGate, key_byte, resume_slot, typed_text,
};
use muda::MenuEvent;
use objc2::MainThreadMarker;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::platform::macos::{
    ActiveEventLoopExtMacOS, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS,
};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowId};

use crate::AppSpec;
use crate::blit::{LETTERBOX, blit_scaled, fill_letterbox};
use crate::frame_timing::{
    FrameTimes, ModernTimes, PresentProbe, ProbeStep, Profiler, parse_probe, parse_probe_size,
    probe_warnings,
};
use crate::layer_present::{LayerPresenter, presented};
use crate::look_switch::{Look, ShapeQueue, look_from_env};
use crate::menus::{MenuBar, MenuCommand, MenuState, menu_update, scales_enabled};
use crate::modern_stage::{ModernStage, escape_closes_sheet};
use crate::motion_watch::{MotionObserver, MotionWatch, SystemMotion};
use crate::pasteboard;
use crate::press_gate::{self, Handled, PressGate, place};
use crate::progress_file::ProgressFile;
use crate::snapshot::{self, ModernShot};
use crate::typeset::TypesetCache;
use crate::user_defaults::Preferences;
use crate::viewport::{DeviceRect, Viewport, window_size};
use crate::wake_schedule::wake_flow;
use crate::window_shape::{
    FullscreenExitObserver, apply_shape, centre_window, easel_for, shape_for, theme_for,
    visible_frame,
};

/// The whole frame, as a damage rect.
const FULL_FRAME: Rect = Rect::new(0, 0, FRAME_WIDTH as i32, FRAME_HEIGHT as i32);

/// Sizes the window's canvas to `len` pixels, as halation's `make_room` sizes its buffers: an
/// unchanged length touches nothing (the zero-damage skip relies on it); a new one drops the old
/// pixels (the next paint is whole: classic's `Full`, modern's cold paint) and fills with
/// `fill`. The capacity is exact, and re-made when it is too small or more than twice the need,
/// so a live resize never leaves a doubled buffer behind (final review N1).
fn fit_canvas(canvas: &mut Vec<u32>, len: usize, fill: u32) {
    if len == canvas.len() {
        return;
    }
    canvas.clear();
    if len > canvas.capacity() || len < canvas.capacity() / 2 {
        *canvas = Vec::new();
        canvas.reserve_exact(len);
    }
    canvas.resize(len, fill);
}

/// What one paint measured, in the look it painted: classic's times and the damage it drew, or
/// modern's times and the stage's damage rects.
enum PaintTimes {
    Classic(FrameTimes, Damage),
    Modern(ModernTimes, usize),
}

/// Events sent to the loop from outside winit: menu commands, the system's reduced-motion
/// setting changing, and AppKit having taken the window out of fullscreen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserEvent {
    Menu(MenuCommand),
    MotionChanged,
    FullscreenExited,
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

    let snapshot = env::var_os("CHROMATRON_SNAPSHOT").map(PathBuf::from);
    let probe = match env::var("CHROMATRON_PRESENT_PROBE") {
        Ok(v) => match parse_probe(&v) {
            Ok(n) => Some(PresentProbe::new(
                n,
                env::var_os("CHROMATRON_PRESENT_PROBE_SWITCH").is_some_and(|s| !s.is_empty()),
            )),
            Err(e) => {
                log::error!("{e}");
                return ExitCode::FAILURE;
            }
        },
        Err(_) => None,
    };
    if snapshot.is_some() && probe.is_some() {
        log::error!("CHROMATRON_SNAPSHOT and CHROMATRON_PRESENT_PROBE are mutually exclusive");
        return ExitCode::FAILURE;
    }
    let probe_size = match probe
        .as_ref()
        .map(|_| parse_probe_size(env::var("CHROMATRON_PROBE_SIZE").ok().as_deref()))
    {
        Some(Ok(size)) => size,
        Some(Err(e)) => {
            log::error!("{e}");
            return ExitCode::FAILURE;
        }
        None => None,
    };
    // A snapshot or probe run never touches the player's files or preferences.
    let dry = snapshot.is_some() || probe.is_some();
    let look = if dry {
        match look_from_env(env::var("CHROMATRON_LOOK").ok().as_deref()) {
            Ok(look) => look,
            Err(e) => {
                log::error!("{e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        Preferences::open(spec.bundle_id).look()
    };
    if probe_size.is_some() && look == Look::Classic {
        log::error!("CHROMATRON_PROBE_SIZE is for the modern look; classic's window is fixed");
        return ExitCode::FAILURE;
    }
    if let Some(path) = &snapshot {
        let result = match look {
            // Ruling P9: before any window, menu or event loop.
            Look::Modern => ModernShot::from_env().and_then(|shot| {
                let scene = snapshot::Scene::from_env()?;
                snapshot::shoot_modern(spec.data, &scene, &shot, path)
            }),
            Look::Classic => {
                snapshot::classic_size_check(env::var("CHROMATRON_SNAPSHOT_SIZE").ok().as_deref())
            }
        };
        match (result, look) {
            (Err(e), _) => {
                log::error!("snapshot: {e}");
                return ExitCode::FAILURE;
            }
            (Ok(()), Look::Modern) => return ExitCode::SUCCESS,
            (Ok(()), Look::Classic) => {}
        }
    }

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
    let motion_proxy = event_loop.create_proxy();
    // Fails only once the loop has exited, when nothing is left to do.
    let observer = (!dry).then(|| {
        let main = MainThreadMarker::new().expect("the event loop runs on the main thread");
        MotionObserver::install(main, move || {
            let _ = motion_proxy.send_event(UserEvent::MotionChanged);
        })
    });

    let progress = ProgressFile::for_app(spec.support_dir, lumen::RuleSet::of(spec.data.game));
    if progress.is_none() {
        log::warn!("no home folder: progress will not be saved");
    }
    let save = match (&progress, dry) {
        (Some(file), false) => file.load(),
        _ => lumen::SaveData::fresh(lumen::RuleSet::of(spec.data.game)),
    };
    let mut app = App::new(
        spec,
        save,
        progress,
        snapshot,
        probe,
        Profiler::new(profiling, launched),
        look,
    );
    app._motion_observer = observer;
    app.probe_size = probe_size;
    app.probe_damage = app.probe.is_some()
        && env::var_os("CHROMATRON_PRESENT_PROBE_DAMAGE").is_some_and(|s| !s.is_empty());
    app.proxy = Some(event_loop.create_proxy());
    if app.snapshot.is_some() {
        // A classic snapshot (a modern one returned above). The first paint is still a full
        // repaint: `damage` starts as `Full` and only the state changes here.
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
    /// Shows the canvas as the window's layer contents (spec §14 "Present", R2).
    presenter: Option<LayerPresenter>,
    /// The 640 × 480 frame `lumen::render` draws into.
    frame: Vec<u32>,
    /// The window's pixels: the upscaled frame, letterbox and text. Kept between presents so a
    /// partial repaint only touches its rects.
    canvas: Vec<u32>,
    viewport: Viewport,
    typeset: TypesetCache,
    damage: Damage,
    latch: ButtonLatch,
    /// The buttons held and the cursor in physical px (ruling P11).
    gate: PressGate,
    /// The system's reduced-motion setting, last read.
    motion: MotionWatch<SystemMotion>,
    /// Keeps the system's accessibility notification observed.
    _motion_observer: Option<MotionObserver>,
    /// Wakes the loop from the window's AppKit notifications.
    proxy: Option<EventLoopProxy<UserEvent>>,
    /// Keeps the window's fullscreen exit observed (a look's shape waits for it). `None` when
    /// winit gave no AppKit handle: the shape then waits only for `fullscreen()` to clear.
    fullscreen_exit: Option<FullscreenExitObserver>,
    modifiers: ModifiersState,
    wheel: WheelGate,
    /// The origin of the wheel gate's and the lightbox's millisecond clock.
    clock: Instant,
    /// Time in `GameState::handle` since the last present.
    handle_time: Duration,
    profiler: Profiler,
    snapshot: Option<PathBuf>,
    snapshot_written: bool,
    probe: Option<PresentProbe>,
    /// A modern probe window's content size (pt), from `CHROMATRON_PROBE_SIZE`.
    probe_size: Option<(f64, f64)>,
    /// `CHROMATRON_PRESENT_PROBE_DAMAGE`: the probe presents the stage's damage, moving a
    /// level's pieces and back, instead of the whole canvas every frame.
    probe_damage: bool,
    /// When the last present finished (the probe's interval between presents).
    last_present: Option<Instant>,
    /// Whether the window was ever reported occluded (the probe warns about it).
    occluded: bool,
    exit_code: ExitCode,
    /// The state the menus currently show.
    shown_menu: Option<MenuState>,
    /// Which look the window shows.
    look: Look,
    /// The modern look's lightbox and captions, made when the modern window first lays out.
    stage: Option<ModernStage>,
    /// The modern theme, from the window's appearance at launch.
    theme: Theme,
    /// How the loop sleeps after this round: `Wait`, or `WaitUntil` a modern cue's next frame.
    flow: ControlFlow,
    /// A look's window shape waiting for fullscreen to end.
    shapes: ShapeQueue,
}

impl App {
    fn new(
        spec: &'static AppSpec,
        save: lumen::SaveData,
        progress: Option<ProgressFile>,
        snapshot: Option<PathBuf>,
        probe: Option<PresentProbe>,
        profiler: Profiler,
        look: Look,
    ) -> App {
        let prefs = Preferences::open(spec.bundle_id);
        let mut state = GameState::new(spec.data, save);
        // Deliberate modern touch (the originals always opened on slot 0). Snapshots and
        // probes ignore it.
        if snapshot.is_none() && probe.is_none() {
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
            presenter: None,
            frame: vec![0; FRAME_WIDTH * FRAME_HEIGHT],
            canvas: Vec::new(),
            viewport: Viewport::fit(FRAME_WIDTH, FRAME_HEIGHT),
            typeset: TypesetCache::default(),
            damage: Damage::Full,
            latch: ButtonLatch::default(),
            gate: PressGate::default(),
            motion: MotionWatch::new(SystemMotion),
            _motion_observer: None,
            proxy: None,
            fullscreen_exit: None,
            modifiers: ModifiersState::empty(),
            wheel: WheelGate::default(),
            clock: Instant::now(),
            handle_time: Duration::ZERO,
            profiler,
            snapshot,
            snapshot_written: false,
            probe,
            probe_size: None,
            probe_damage: false,
            last_present: None,
            occluded: false,
            exit_code: ExitCode::SUCCESS,
            shown_menu: None,
            look,
            stage: None,
            theme: Theme::Dark,
            flow: ControlFlow::Wait,
            shapes: ShapeQueue::default(),
        }
    }

    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        self.menus = Some(MenuBar::install(self.wheel_on, self.look == Look::Classic));
        let mut shape = shape_for(self.look, visible_frame());
        if let Some((w, h)) = self.probe_size.filter(|_| self.look == Look::Modern) {
            shape.size = LogicalSize::new(w, h);
        }
        let mut attributes = Window::default_attributes()
            .with_title(self.spec.data.ui.window_title.as_ref())
            .with_inner_size(shape.size)
            .with_resizable(shape.resizable)
            .with_enabled_buttons(shape.buttons)
            .with_accepts_first_mouse(false);
        if let Some(min) = shape.min_size {
            attributes = attributes.with_min_inner_size(min);
        }
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("cannot open the window: {e}"))?,
        );
        centre_window(&window);
        if let Some(proxy) = self.proxy.clone() {
            let main = MainThreadMarker::new().expect("the event loop runs on the main thread");
            // Fails only once the loop has exited, when nothing is left to do.
            self.fullscreen_exit = FullscreenExitObserver::install(main, &window, move || {
                let _ = proxy.send_event(UserEvent::FullscreenExited);
            });
        }
        self.theme = theme_for(window.theme());
        self.presenter = Some(
            LayerPresenter::new(&window)
                .map_err(|e| format!("cannot draw into the window: {e}"))?,
        );
        let size = window.inner_size();
        self.window = Some(window);
        self.resized(size);
        Ok(())
    }

    /// The window's content changed size (open, scale, fullscreen, another display, a modern
    /// resize, a look switch).
    fn resized(&mut self, size: PhysicalSize<u32>) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let width = NonZeroU32::new(size.width).unwrap_or(NonZeroU32::MIN);
        let height = NonZeroU32::new(size.height).unwrap_or(NonZeroU32::MIN);
        let scale = window.scale_factor();
        let points = PhysicalSize::new(width.get(), height.get()).to_logical::<f64>(scale);
        if let Some(presenter) = &mut self.presenter
            && let Err(e) = presenter.resize(
                width.get() as usize,
                height.get() as usize,
                scale,
                (points.width, points.height),
            )
        {
            // The canvas and stage still follow the window (review M5); presents fail until a
            // later `Resized` makes surfaces.
            log::error!("cannot resize the surface: {e}");
        }
        self.viewport = Viewport::fit(width.get() as usize, height.get() as usize);
        match self.look {
            Look::Classic => {
                fit_canvas(
                    &mut self.canvas,
                    self.viewport.width * self.viewport.height,
                    LETTERBOX,
                );
                self.damage.add(lumen::Redraw::Full);
            }
            Look::Modern => {
                // A resize ends any drag (ruling P6).
                self.pointer_cancel();
                let easel = easel_for(
                    PhysicalSize::new(width.get(), height.get()),
                    window.scale_factor(),
                );
                fit_canvas(
                    &mut self.canvas,
                    easel.width as usize * easel.height as usize,
                    0,
                );
                match &mut self.stage {
                    // Ruling P6: only a new easel re-arranges (and paints cold).
                    Some(stage) if stage.lightbox().easel() != &easel => stage.resize(easel),
                    Some(_) => {}
                    None => {
                        self.stage =
                            Some(ModernStage::new(easel, self.theme, self.motion.current()))
                    }
                }
            }
        }
        window.request_redraw();
        self.sync_menus();
    }

    /// The window's real fullscreen state, scale, look and Levels…, as the menus show them.
    fn menu_state(&self) -> Option<MenuState> {
        let window = self.window.as_ref()?;
        Some(MenuState {
            fullscreen: window.fullscreen().is_some(),
            leaving: self.shapes.is_leaving(),
            scale: self.viewport.scale,
            classic: self.look == Look::Classic,
            levels: self.look == Look::Modern
                && self.stage.as_ref().is_some_and(|s| !s.has_inline_picker()),
        })
    }

    /// Shows the menu state when it changed.
    fn sync_menus(&mut self) {
        let (Some(now), Some(menus)) = (self.menu_state(), &self.menus) else {
            return;
        };
        if let Some(state) = menu_update(self.shown_menu, now) {
            menus.show(&state);
            self.shown_menu = Some(state);
        }
    }

    /// The cursor in the current look's units, read now (ruling P11).
    fn at(&self) -> (i32, i32) {
        place(self.look, &self.viewport, self.gate.cursor())
    }

    /// Carries out what an input did: the slot remembered, the save written, the clipboard
    /// set, and a repaint when the picture changed.
    fn apply(&mut self, slot: usize, handled: Handled) {
        let Handled {
            effects,
            sheet_changed,
        } = handled;
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
        if (!self.damage.is_none() || sheet_changed)
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        if sheet_changed {
            self.sync_menus();
        }
    }

    /// A pointer event, through the canvas's controls in modern (spec §5.3).
    fn route(&mut self, event: InputEvent) {
        let started = Instant::now();
        let slot = self.state.slot();
        let modern = self.look == Look::Modern;
        let handled = press_gate::route(
            self.stage.as_mut().filter(|_| modern),
            &mut self.state,
            event,
        );
        self.handle_time += started.elapsed();
        self.apply(slot, handled);
    }

    /// A key, the wheel or a menu's key, straight to the game.
    fn send(&mut self, event: InputEvent) {
        let started = Instant::now();
        let slot = self.state.slot();
        let modern = self.look == Look::Modern;
        let effects = press_gate::send(
            self.stage.as_ref().filter(|_| modern),
            &mut self.state,
            event,
        );
        self.handle_time += started.elapsed();
        self.apply(
            slot,
            Handled {
                effects,
                sheet_changed: false,
            },
        );
    }

    /// The pointer was lost (focus, a resize, a look switch): `press_gate::cancel`.
    fn pointer_cancel(&mut self) {
        let slot = self.state.slot();
        let modern = self.look == Look::Modern;
        let effects = press_gate::cancel(
            &mut self.gate,
            self.stage.as_mut().filter(|_| modern),
            &mut self.state,
        );
        self.apply(
            slot,
            Handled {
                effects,
                sheet_changed: false,
            },
        );
    }

    /// Spec §11, §13: switches look. The drag ends (through the old look's floorplan); the
    /// choice is stored (never in a dry run); a modern lightbox closes its sheet and paints
    /// cold; the window takes the look's shape, centred, now or once fullscreen ends.
    fn set_look(&mut self, look: Look) {
        if look != self.look {
            self.pointer_cancel();
            let from = std::mem::replace(&mut self.look, look);
            if !self.dry() {
                self.prefs.set_classic_look(look == Look::Classic);
            }
            if let Some(stage) = &mut self.stage {
                stage.command(Control::CloseSheet);
                stage.reset();
            }
            self.flow = ControlFlow::Wait;
            self.damage.add(lumen::Redraw::Full);
            if let Some(window) = self.window.clone() {
                match self
                    .shapes
                    .request(from, look, window.fullscreen().is_some())
                {
                    Some(look) => self.reshape(&window, look),
                    // Lay out at the size the window has now (fullscreen, or on its way out);
                    // a `Resized` that follows is skipped by `resized` when the easel didn't
                    // change (ruling P6). The shape waits for `fullscreen_exited`.
                    None => self.resized(window.inner_size()),
                }
            }
        }
        // muda flips a check item when it is clicked; show the real state again.
        self.shown_menu = None;
        self.sync_menus();
    }

    /// Gives the window `look`'s shape (spec §13), centred (ruling P12), and lays out at the
    /// size it has now. A zoomed frame is simply overwritten by the shape's size (measured:
    /// classic ends 640 × 480, not resizable, not zoomed), so the window moves once.
    fn reshape(&mut self, window: &Window, look: Look) {
        apply_shape(window, &shape_for(look, visible_frame()));
        centre_window(window);
        self.resized(window.inner_size());
    }

    /// AppKit finished taking the window out of fullscreen and restored its frame: a look
    /// switched meanwhile takes its shape now (spec §13), unless ⌃⌘F during the exit has
    /// already sent the window back into fullscreen (winit replays it inside the exit).
    fn fullscreen_exited(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if let Some(look) = self.shapes.exited(window.fullscreen().is_some()) {
            self.reshape(&window, look);
        }
    }

    /// Escape over the sheet, or File ▸ Levels… (ruling P10): a held press ends, then the
    /// sheet closes or toggles.
    fn sheet(&mut self, control: Control) {
        if self.look != Look::Modern {
            return;
        }
        let Some(stage) = self.stage.as_mut() else {
            return;
        };
        let slot = self.state.slot();
        let handled = press_gate::sheet_command(&mut self.gate, stage, &mut self.state, control);
        self.apply(slot, handled);
    }

    /// Spec §9.3: the system's reduced-motion setting, read again; a change settles any cue.
    fn refresh_motion(&mut self) {
        if let Some(on) = self.motion.refresh()
            && let Some(stage) = &mut self.stage
        {
            stage.set_reduced_motion(on);
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    /// A damage probe's move (review M4), through the real input path: the level's stored
    /// solution pasted, or, once it is won, the level reset; each plays its cues, painted
    /// through the stage's damage.
    fn probe_move(&mut self) {
        if self.state.is_won() {
            self.send(InputEvent::Key(b'r'));
        } else if let Some(text) = lumen::stored_solution(self.state.data(), self.state.slot()) {
            self.send(InputEvent::Paste(text));
        }
        // Even a move that changed nothing paints again, so the probe never stalls.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// A run that leaves the player's files and preferences alone (snapshot, probe).
    fn dry(&self) -> bool {
        self.snapshot.is_some() || self.probe.is_some()
    }

    /// Stores the current level for the next launch (never in a dry run).
    fn remember_slot(&self) {
        if !self.dry() {
            self.prefs.set_last_level_slot(self.state.slot());
        }
    }

    fn write_save(&self) {
        if self.dry() {
            return;
        }
        if let Some(file) = &self.progress
            && let Err(e) = file.store(&self.state.save_bytes())
        {
            log::error!("cannot write {}: {e}", file.path().display());
        }
    }

    /// The clock the lightbox animates on (ms).
    fn now_ms(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    /// Brings the classic canvas up to date with the pending damage.
    fn draw_classic(&mut self) -> (FrameTimes, Damage) {
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
        (times, damage)
    }

    /// Spec §13: paint the lightbox, ink the captions; the next wake follows the cues.
    fn draw_modern(&mut self) -> (ModernTimes, usize) {
        let now = self.now_ms();
        let trace = std::mem::take(&mut self.handle_time);
        // The lightbox finds its own damage from the state; classic's is not needed.
        self.damage.take();
        let Some(stage) = &mut self.stage else {
            return (ModernTimes::default(), 0);
        };
        let times = stage.paint(&self.state, now, &mut self.canvas);
        self.flow = wake_flow(Look::Modern, stage.next_wake(now), now, self.clock);
        let rects = stage.damage().len();
        (
            ModernTimes {
                trace,
                paint: times.paint,
                captions: times.captions,
                present: Duration::ZERO,
            },
            rects,
        )
    }

    /// Brings the canvas up to date in the window's look and presents it.
    fn paint(&mut self, event_loop: &ActiveEventLoop) {
        if self.presenter.is_none() {
            return;
        }
        let measured = match self.look {
            Look::Classic => {
                self.flow = ControlFlow::Wait;
                let (times, damage) = self.draw_classic();
                PaintTimes::Classic(times, damage)
            }
            Look::Modern => {
                let (times, rects) = self.draw_modern();
                PaintTimes::Modern(times, rects)
            }
        };
        // The probe presents the whole canvas (the worst case, a cold paint's), unless it
        // probes the stage's damage (the interactive path).
        let everything = matches!(measured, PaintTimes::Classic(..))
            || self.snapshot.is_some()
            || (self.probe.is_some() && !self.probe_damage);
        let whole = halation::DeviceRect::new(
            0,
            0,
            self.viewport.width as i32,
            self.viewport.height as i32,
        );
        let damage = self.stage.as_ref().map_or(&[][..], ModernStage::damage);
        let Some(presenter) = &mut self.presenter else {
            return;
        };
        let started = Instant::now();
        // `shown`: the layer shows the canvas; `present`: the time of a present that ran.
        let (shown, present) = match presented(&whole, everything, damage, presenter.is_current()) {
            None => (true, None),
            Some(rects) => match presenter.present(&self.canvas, rects) {
                Ok(()) => (true, Some(started.elapsed())),
                Err(e) => {
                    log::error!("cannot present: {e}");
                    (false, None)
                }
            },
        };
        match measured {
            PaintTimes::Classic(mut times, damage) => {
                times.present = present.unwrap_or_default();
                self.profiler.frame(times, damage);
            }
            PaintTimes::Modern(mut times, rects) => {
                times.present = present.unwrap_or_default();
                self.profiler.modern_frame(times, rects);
            }
        }

        // The step first, so an arm may borrow the whole `App` (Task 13's look switch). The
        // interval is between presents: a damage probe's paint that presented nothing is not one.
        let gap = present.and_then(|_| {
            let now = Instant::now();
            self.last_present.replace(now).map(|last| now - last)
        });
        let step = self.probe.as_mut().map(|probe| {
            if let Some(gap) = gap {
                probe.record_gap(gap);
            }
            probe.record(present)
        });
        match step {
            Some(ProbeStep::Done) => {
                if let Some(s) = self.probe.as_ref().map(PresentProbe::summary) {
                    let size = self.window.as_ref().map(|w| w.inner_size());
                    let (w, h) = size.map_or((0, 0), |s| (s.width, s.height));
                    let visible = self.window.as_ref().and_then(|w| w.is_visible());
                    // Read back: the surface the layer shows holds the canvas, alpha opaque.
                    let holds = self
                        .presenter
                        .as_ref()
                        .map(|p| p.shows(&self.canvas))
                        .unwrap_or(Ok(false))
                        .unwrap_or_else(|e| {
                            log::error!("{e}");
                            false
                        });
                    // Review M3: the surfaces' memory, beside the canvas's.
                    let (surfaces, bytes, crowded) =
                        self.presenter.as_ref().map_or((0, 0, 0), |p| {
                            let (n, bytes) = p.memory();
                            (n, bytes, p.crowded())
                        });
                    let mb = |b: usize| b as f64 / 1_000_000.0;
                    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
                    let via = if self.probe_damage {
                        ", through the stage's damage"
                    } else {
                        ""
                    };
                    // A switching run's main figures are the presents before the switch; the
                    // ones after it (the other look's) are reported apart.
                    let (switched, before, after) = match s.after_switch {
                        Some(a) => (
                            ", after a switch",
                            "present before the switch",
                            format!(
                                "; after the switch p50 {:.2} ms, p90 {:.2} ms, max {:.2} ms",
                                ms(a.p50),
                                ms(a.p90),
                                ms(a.max)
                            ),
                        ),
                        None => ("", "present", String::new()),
                    };
                    println!(
                        "present probe ({:?} look{switched}{via}, {w}×{h} px, {} frames, {} presented): {before} p50 {:.2} ms, p90 {:.2} ms, max {:.2} ms{after}; interval p50 {:.2} ms, p90 {:.2} ms; visible {visible:?}, occluded {}; layer holds the canvas {holds}; {surfaces} surfaces, {:.1} MB (canvas {:.1} MB), {crowded} presents into a surface in use",
                        self.look,
                        s.frames,
                        s.presented,
                        ms(s.present.p50),
                        ms(s.present.p90),
                        ms(s.present.max),
                        ms(s.gap_p50),
                        ms(s.gap_p90),
                        self.occluded,
                        mb(bytes),
                        mb(self.canvas.capacity() * 4),
                    );
                    let hidden = self.occluded || visible == Some(false);
                    for warning in probe_warnings(hidden, s.presented).into_iter().flatten() {
                        println!("warning: {warning}");
                    }
                }
                event_loop.exit();
            }
            // A damage probe paints at the cues' own pace, and moves once they settle.
            Some(ProbeStep::Next) if self.probe_damage => {
                if self.flow == ControlFlow::Wait {
                    self.probe_move();
                }
            }
            Some(ProbeStep::Next) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Some(ProbeStep::SwitchLook) => {
                self.set_look(self.look.other());
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            None => {}
        }

        if self.snapshot.is_some() && !shown {
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
                    // winit reports no fullscreen from here, before AppKit's exit has run: a
                    // look's shape keeps waiting for `fullscreen_exited` (only if the exit is
                    // observed, or it would wait for ever).
                    if next.is_none() && self.fullscreen_exit.is_some() {
                        self.shapes.leaving();
                    }
                    window.set_fullscreen(next);
                }
            }
            MenuCommand::Scale(scale) => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                // The items are disabled where they don't apply (modern, fullscreen, and View ▸
                // Exit Full Screen's animation, when AppKit still owns the frame); the same rule
                // guards a stray event.
                if self.menu_state().as_ref().is_some_and(scales_enabled) {
                    let (width, height) = window_size(scale);
                    if let Some(size) = window.request_inner_size(PhysicalSize::new(width, height))
                    {
                        self.resized(size);
                    }
                }
                // muda flips a check item when it is clicked; show the real state again.
                self.shown_menu = None;
                self.sync_menus();
            }
            MenuCommand::ToggleClassicLook => self.set_look(self.look.other()),
            MenuCommand::Levels => self.sheet(Control::Levels),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // At rest nothing animates (platform.md P9); a modern cue sets `WaitUntil` (spec §13).
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
            WindowEvent::Focused(false) => self.pointer_cancel(),
            WindowEvent::Focused(true) => self.refresh_motion(),
            WindowEvent::ThemeChanged(appearance) => {
                let theme = theme_for(Some(appearance));
                if theme != self.theme {
                    self.theme = theme;
                    if let Some(stage) = &mut self.stage {
                        stage.set_theme(theme);
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Occluded(hidden) => self.occluded |= hidden,
            WindowEvent::RedrawRequested => self.paint(event_loop),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.gate.moved(position.x, position.y);
                // Deliberate deviation: the original dropped drags outside the content area
                // (platform.md P4). We keep forwarding them and lumen clips; harmless.
                if self.gate.held() {
                    let (x, y) = self.at();
                    self.route(InputEvent::MouseDragged { x, y });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Right => Button::Secondary,
                    _ => Button::Other,
                };
                match state {
                    ElementState::Pressed => {
                        self.gate.press();
                        let held = Held {
                            shift: self.modifiers.shift_key(),
                            option: self.modifiers.alt_key(),
                            control: self.modifiers.control_key(),
                        };
                        let alt = self.latch.press(button, held);
                        let (x, y) = self.at();
                        self.route(InputEvent::MouseDown { x, y, alt });
                    }
                    ElementState::Released => {
                        // A release without a press is the window-activating click's mouse-up
                        // (or one after a cancel): drop it, or it would rotate a piece.
                        if !self.gate.release() {
                            return;
                        }
                        let alt = self.latch.release(button);
                        let (x, y) = self.at();
                        self.route(InputEvent::MouseUp { x, y, alt });
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
                if let Some(delta) = self
                    .wheel
                    .feed(scroll, self.clock.elapsed().as_secs_f64() * 1000.0)
                {
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
                let byte = key_byte(typed, self.modifiers.super_key(), event.repeat);
                let sheet = self.look == Look::Modern
                    && self.stage.as_ref().is_some_and(ModernStage::sheet_open);
                // Ruling P10: Escape closes an open sheet before the game sees it.
                if escape_closes_sheet(byte, sheet) {
                    self.sheet(Control::CloseSheet);
                } else if let Some(byte) = byte {
                    self.send(InputEvent::Key(byte));
                }
            }
            _ => {}
        }
    }

    /// Spec §13: a modern cue's next frame is due. `Wait` first, so a redraw that never comes
    /// leaves the loop asleep; the paint arms the next wake.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.flow = ControlFlow::Wait;
            if self.look == Look::Modern
                && let Some(window) = &self.window
            {
                window.request_redraw();
            }
        }
    }

    /// The menus follow winit's fullscreen state, which changes with no `Resized` after it:
    /// at once when View ▸ Full Screen asks to leave (before AppKit's exit animation), and only
    /// at the end of the animation for the green button or the system's shortcut. (A look's
    /// shape waits for AppKit's exit itself: `fullscreen_exited`.) The loop then sleeps as the
    /// last paint (or wake) decided.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Final review N3: with no exit observer no exit ever arrives, so a waiting shape is
        // taken once winit reports no fullscreen (the behaviour before the observer).
        if self.fullscreen_exit.is_none()
            && self
                .window
                .as_ref()
                .is_some_and(|w| w.fullscreen().is_none())
        {
            self.fullscreen_exited();
        }
        self.sync_menus();
        event_loop.set_control_flow(self.flow);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Menu(command) => self.menu(event_loop, command),
            UserEvent::MotionChanged => self.refresh_motion(),
            UserEvent::FullscreenExited => self.fullscreen_exited(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Final review N1: a live resize that grows a few px at a time, then shrinks, keeps the
    /// canvas at one picture's worth (2560 × 1600 grows to 2600 × 1640, back to 1280 × 800).
    #[test]
    fn the_canvas_tracks_the_window_through_a_live_resize() {
        let mut canvas = Vec::new();
        for (w, h) in (0..=40).step_by(4).map(|d| (2560 + d, 1600 + d)) {
            fit_canvas(&mut canvas, w * h, 7);
            assert_eq!(canvas.len(), w * h);
            assert_eq!(canvas.capacity(), w * h, "{w}×{h}: exact, not doubled");
            assert!(canvas.iter().all(|&p| p == 7), "{w}×{h}: filled");
        }
        fit_canvas(&mut canvas, 2590 * 1630, 7);
        assert_eq!(
            canvas.capacity(),
            2600 * 1640,
            "a small shrink keeps the buffer"
        );
        fit_canvas(&mut canvas, 1280 * 800, 7);
        assert_eq!(
            canvas.capacity(),
            1280 * 800,
            "a large shrink gives it back"
        );
    }

    /// An unchanged size leaves the pixels alone: the presenter's zero-damage skip shows them.
    #[test]
    fn an_unchanged_size_keeps_the_canvas() {
        let mut canvas = Vec::new();
        fit_canvas(&mut canvas, 640 * 480, LETTERBOX);
        canvas[123] = 0x00ab_cdef;
        let at = canvas.as_ptr();
        fit_canvas(&mut canvas, 640 * 480, LETTERBOX);
        assert_eq!((canvas[123], canvas.as_ptr()), (0x00ab_cdef, at));
    }
}
