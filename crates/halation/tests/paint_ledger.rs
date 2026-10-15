//! Phase 6's pixel ledger (plan 13, ruling G1): an FNV-1a hash of each paint in a fixed set,
//! settled and mid-cue, in both themes, one line per paint. A speed change must leave every
//! line as it was. Ignored by `just test`; run it as
//! `cargo test -p halation --test paint_ledger -- --ignored --nocapture`.

mod common;

use common::SETTLED;
use halation::{Control, DeviceRect, Easel, Lightbox, Output, Tableau, Theme};
use lumen::{
    BOARD_CELLS, Board, GameState, InputEvent, Piece, SaveData, VISIBLE_TOOLBOX_SLOTS, board_index,
    stored_solution, toolbox_index,
};
use spectrograph::Game;

/// (width, height, px per pt, touch): the bench's sizes, a px wider, the web's reference size, a
/// 1× window, a light tile past 1800 px (s < 1), a sheet layout, and the phones both ways.
const EASELS: [(u32, u32, f32, bool); 9] = [
    (2560, 1600, 2.0, false),
    (2561, 1600, 2.0, false),
    (2450, 1671, 1.75, false),
    (1281, 800, 1.0, false),
    (3456, 2234, 2.0, false),
    (1600, 1200, 2.0, false),
    (780, 1480, 2.0, true),
    (781, 1688, 2.0, true),
    (1688, 780, 2.0, true),
];

/// The cue sets, (easel, through `Rgba`): the bench's desktop and phone, the landscape phone, the
/// web's reference size (radius-4 glow windows, the web's overlays and fade), the light tile at
/// s < 1 and the 1× window (their own windowed develops; review I1), and the web phone (its
/// held piece at 1.3× lift, its drop ring and fade through `RgbaBytes`).
const CUE_SETS: [((u32, u32, f32, bool), bool); 7] = [
    (EASELS[0], false),
    (EASELS[6], false),
    (EASELS[8], false),
    (EASELS[2], true),
    (EASELS[4], false),
    (EASELS[3], false),
    (EASELS[6], true),
];

fn easel((w, h, s, touch): (u32, u32, f32, bool)) -> Easel {
    Easel::new(w, h, s).with_touch(touch)
}

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u32 {
    bytes.into_iter().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

fn hash(px: &[u32]) -> u32 {
    fnv(px.iter().flat_map(|p| p.to_le_bytes()))
}

/// What a line is about: the theme the set runs in, the game and its slot.
#[derive(Clone, Copy)]
struct Run {
    theme: Theme,
    game: Game,
    slot: usize,
}

fn line(kind: &str, run: Run, e: Easel, what: &str, h: u32) {
    let t = if e.touch { "t" } else { "" };
    println!(
        "{kind} {:?} C{} s{} {}x{}@{}{t} {what} {h:08x}",
        run.theme,
        run.game.number(),
        run.slot,
        e.width,
        e.height,
        e.px_per_pt
    );
}

/// The slot, of the first 50, with the highest `score` (the first of equals), as the bench picks.
fn pick(game: Game, score: impl Fn(&Board) -> usize) -> usize {
    let data = common::data(game);
    (0..data.level_order.len().min(50))
        .max_by_key(|&s| {
            let board = Board::from_level(data.slot_level(s).unwrap());
            (score(&board), usize::MAX - s)
        })
        .unwrap()
}

fn densest(game: Game) -> usize {
    pick(game, |b| {
        b.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece != Piece::Empty)
            .count()
    })
}

fn slots(game: Game) -> Vec<usize> {
    let walls = pick(game, |b| {
        b.cells[..BOARD_CELLS]
            .iter()
            .filter(|c| c.piece == Piece::Wall)
            .count()
    });
    let mut s = vec![0, 20, 41, densest(game), walls];
    s.sort_unstable();
    s.dedup();
    s
}

/// `slot` of `game`, solved with its stored solution where one exists. Unlike `common::state`
/// it neither needs a stored solution nor asserts the win: the ledger records what is painted.
fn level(game: Game, slot: usize) -> GameState<'static> {
    let data = common::data(game);
    let mut state = GameState::new(data, SaveData::default());
    state.goto_slot(slot);
    if let Some(text) = stored_solution(data, slot) {
        state.handle(InputEvent::Paste(text));
    }
    state
}

fn centre_of(tableau: &Tableau, index: usize) -> (i32, i32) {
    let r = tableau.cell_rect(index);
    ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
}

/// The last empty board cell: a drop target with its ring.
fn last_empty(state: &GameState) -> Option<usize> {
    (0..BOARD_CELLS)
        .rev()
        .find(|&i| state.board().cells[i].piece == Piece::Empty)
}

/// The first visible tray slot (as a cell index) whose piece passes `wanted`.
fn tray_slot(state: &GameState, wanted: impl Fn(Piece) -> bool) -> Option<usize> {
    (0..VISIBLE_TOOLBOX_SLOTS)
        .map(toolbox_index)
        .find(|&i| wanted(state.board().cells[i].piece))
}

/// An output buffer of either kind: the macOS canvas (`Xrgb`) or the web's `ImageData` (`Rgba`),
/// so every cue runs through both targets (review I3). The `usize` is the width in px.
enum Buf {
    Xrgb(Vec<u32>, usize),
    Rgba(Vec<u8>, usize),
}

impl Buf {
    fn new(e: Easel, rgba: bool) -> Buf {
        let (w, n) = (e.width as usize, (e.width * e.height) as usize);
        if rgba {
            Buf::Rgba(vec![0; 4 * n], w)
        } else {
            Buf::Xrgb(vec![0; n], w)
        }
    }
    fn out(&mut self) -> Output<'_> {
        match self {
            Buf::Xrgb(px, _) => Output::Xrgb(px),
            Buf::Rgba(bytes, _) => Output::Rgba(bytes),
        }
    }
    fn hash(&self) -> u32 {
        match self {
            Buf::Xrgb(px, _) => hash(px),
            Buf::Rgba(bytes, _) => fnv(bytes.iter().copied()),
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Buf::Xrgb(..) => "cue",
            Buf::Rgba(..) => "cue-rgba",
        }
    }
    /// Draws over `r` as a shell does (its captions): zeroes, which the next paint must undo.
    fn scribble(&mut self, r: DeviceRect) {
        let (w, h) = match self {
            Buf::Xrgb(px, w) => (*w, px.len() / *w),
            Buf::Rgba(bytes, w) => (*w, bytes.len() / 4 / *w),
        };
        let clamp = |v: i32, max: usize| (v.max(0) as usize).min(max);
        let (x0, x1) = (clamp(r.left, w), clamp(r.right, w).max(clamp(r.left, w)));
        for y in clamp(r.top, h)..clamp(r.bottom, h) {
            match self {
                Buf::Xrgb(px, _) => px[y * w + x0..y * w + x1].fill(0),
                Buf::Rgba(bytes, _) => bytes[4 * (y * w + x0)..4 * (y * w + x1)].fill(0),
            }
        }
    }
}

/// One cue set's lightbox, game and canvas: every paint lands on what the last ones left.
struct Rig {
    lb: Lightbox,
    state: GameState<'static>,
    buf: Buf,
    run: Run,
    /// Each line's `what` starts with this: `reduced:` with reduced motion.
    tag: &'static str,
}

impl Rig {
    fn new(state: GameState<'static>, spec: (u32, u32, f32, bool), rgba: bool, run: Run) -> Rig {
        Rig::with_motion(state, spec, rgba, run, false)
    }

    /// With reduced motion when `reduced` (its lines marked `reduced:`).
    fn with_motion(
        state: GameState<'static>,
        spec: (u32, u32, f32, bool),
        rgba: bool,
        run: Run,
        reduced: bool,
    ) -> Rig {
        let e = easel(spec);
        Rig {
            lb: Lightbox::new(e, run.theme, reduced),
            state,
            buf: Buf::new(e, rgba),
            run,
            tag: if reduced { "reduced:" } else { "" },
        }
    }

    /// A first paint, not hashed: what the cues start from.
    fn start(&mut self) {
        self.lb.paint(&self.state, 0.0, self.buf.out());
    }

    /// One mid-cue frame: painted at `now` onto what the last frames left, then hashed.
    fn shot(&mut self, now: f64, what: &str) {
        self.lb.paint(&self.state, now, self.buf.out());
        let what = format!("{}{what}", self.tag);
        let e = *self.lb.easel();
        line(self.buf.kind(), self.run, e, &what, self.buf.hash());
    }

    fn input(&mut self, event: InputEvent) {
        self.state.handle_on(&self.lb, event);
    }

    /// A press and release with `alt`: a turn.
    fn turn(&mut self, (x, y): (i32, i32)) {
        self.input(InputEvent::MouseDown { x, y, alt: true });
        self.input(InputEvent::MouseUp { x, y, alt: true });
    }

    /// A drag from `from` over `path`, one frame each from `t`, released at the path's end, then
    /// its land at +60 and settled at +2000.
    fn carry(&mut self, t: f64, what: &str, (x, y): (i32, i32), path: &[(i32, i32)]) {
        self.input(InputEvent::MouseDown { x, y, alt: false });
        for (k, &(x, y)) in path.iter().enumerate() {
            self.input(InputEvent::MouseDragged { x, y });
            self.shot(t + 30.0 * k as f64, &format!("{what}{k}"));
        }
        let (x, y) = *path.last().unwrap();
        self.input(InputEvent::MouseUp { x, y, alt: false });
        let up = t + 30.0 * path.len() as f64;
        for dt in [60.0, 2000.0] {
            self.shot(up + dt, &format!("{what}-land+{dt}"));
        }
    }

    /// The theme flipped and back, a cold paint in each.
    fn flip(&mut self, t: f64, what: &str) {
        let theme = self.run.theme;
        let other = if theme == Theme::Dark {
            Theme::Light
        } else {
            Theme::Dark
        };
        self.lb.set_theme(other);
        self.shot(t, what);
        self.lb.set_theme(theme);
        self.shot(t + 10.0, &format!("{what}-back"));
    }
}

/// The cues on `game`'s densest level at one easel and output, with reduced motion when
/// `reduced` (its lines are marked `reduced:`):
/// - a turn's sweep, and a second turn with its piece picked up while it sweeps;
/// - a drag (rising, over an empty cell so its drop ring shows, over the tray, at the canvas
///   corner), with a theme flip and back while held (cold paints mid-drag), and its land back;
/// - a move onto that empty cell, then from there into an empty tray slot (a landing in the
///   tray);
/// - a reset, then a drag from the tray onto the board;
/// - a rect the shell drew over (`invalidate`);
/// - a cheat-key win; a level change (its fade); then a theme flip and back (each a cold,
///   settled repaint in the other theme's planes).
fn cues_at(theme: Theme, game: Game, spec: (u32, u32, f32, bool), rgba: bool, reduced: bool) {
    let slot = densest(game);
    let run = Run { theme, game, slot };
    let mut rig = Rig::with_motion(level(game, slot), spec, rgba, run, reduced);
    rig.start();
    let tableau = rig.lb.tableau().clone();
    let board = rig.state.board().cells;
    let movable = (0..BOARD_CELLS).find(|&i| board[i].piece.is_movable());
    if let (Some(cell), Some(empty)) = (movable, last_empty(&rig.state)) {
        let at = centre_of(&tableau, cell);
        rig.turn(at);
        for dt in [0.0, 17.0, 50.0, 100.0, 200.0, 400.0, 800.0, 2000.0] {
            rig.shot(1000.0 + dt, &format!("turn+{dt}"));
        }
        // Picked up while its own turn sweeps (overlapping damage), set down in its own cell.
        rig.turn(at);
        rig.shot(3000.0, "turn2+0");
        let (x, y) = at;
        rig.carry(3017.0, "sweepdrag", at, &[(x + 4, y + 3), (x + 11, y + 7)]);

        let t0 = 6000.0;
        rig.input(InputEvent::MouseDown { x, y, alt: false });
        let over = centre_of(&tableau, empty);
        let tray = centre_of(&tableau, toolbox_index(0));
        let path = [(x + 9, y + 5), over, (over.0 + 3, over.1 - 2), tray, (2, 2)];
        for (k, (px, py)) in path.into_iter().enumerate() {
            rig.input(InputEvent::MouseDragged { x: px, y: py });
            let t = t0 + 30.0 * k as f64;
            rig.shot(t, &format!("drag{k}"));
            if k == 1 {
                rig.flip(t + 10.0, "drag-flipped");
            }
        }
        rig.input(InputEvent::MouseUp {
            x: 2,
            y: 2,
            alt: false,
        });
        for dt in [60.0, 2000.0] {
            rig.shot(t0 + 200.0 + dt, &format!("land+{dt}"));
        }

        rig.carry(9000.0, "move", at, &[over]);
        if let Some(slot) = tray_slot(&rig.state, |p| p == Piece::Empty) {
            rig.carry(12_000.0, "totray", over, &[centre_of(&tableau, slot)]);
        }
    }
    rig.state.handle(InputEvent::Key(b'r'));
    rig.shot(15_000.0, "reset");
    // After a reset every piece is in the tray.
    let from = tray_slot(&rig.state, Piece::is_movable);
    if let (Some(from), Some(to)) = (from, last_empty(&rig.state)) {
        let (from, to) = (centre_of(&tableau, from), centre_of(&tableau, to));
        rig.carry(16_000.0, "fromtray", from, &[to]);
    }
    let drawn = tableau.cell_rect(toolbox_index(0));
    rig.buf.scribble(drawn);
    rig.lb.invalidate(drawn);
    rig.shot(19_000.0, "invalidated");
    let cheat = rig.state.rules().cheat_key;
    rig.state.handle(InputEvent::Key(cheat));
    if rig.state.is_won() {
        for dt in [120.0, 500.0, 900.0, 3000.0] {
            rig.shot(20_000.0 + dt, &format!("win+{dt}"));
        }
        // The next level where it is unlocked, else the previous one (C1, C2 and C4's densest
        // levels have a locked successor in a fresh save), so every game shows a level fade.
        let was = rig.state.slot();
        rig.state.handle(InputEvent::Key(b'='));
        if rig.state.slot() == was {
            rig.state.handle(InputEvent::Key(b'-'));
        }
        if rig.state.slot() != was {
            for dt in [0.0, 90.0, 400.0, 3000.0] {
                rig.shot(25_000.0 + dt, &format!("level+{dt}"));
            }
        }
    }
    rig.flip(30_000.0, "flipped");
}

/// Every cue set (`CUE_SETS`, and the bench's desktop with reduced motion), and the sheet on a
/// sheet layout in both outputs: its fade in, a win under it (the strip changing while it is
/// open), and its fade out.
fn cues(theme: Theme, game: Game) {
    for (spec, rgba) in CUE_SETS {
        cues_at(theme, game, spec, rgba, false);
    }
    cues_at(theme, game, EASELS[0], false, true);
    let slot = densest(game);
    let run = Run { theme, game, slot };
    for rgba in [false, true] {
        // Unsolved, so the win under the sheet completes its slot and changes the strip.
        let mut state = GameState::new(common::data(game), SaveData::default());
        state.goto_slot(slot);
        let mut rig = Rig::new(state, EASELS[5], rgba, run);
        rig.start();
        assert!(rig.lb.command(Control::Levels), "a sheet layout");
        for dt in [0.0, 50.0, 100.0, 250.0] {
            rig.shot(1000.0 + dt, &format!("Levels+{dt}"));
        }
        let cheat = rig.state.rules().cheat_key;
        rig.state.handle(InputEvent::Key(cheat));
        for dt in [0.0, 120.0, 3000.0] {
            rig.shot(2000.0 + dt, &format!("sheet-won+{dt}"));
        }
        assert!(rig.lb.command(Control::CloseSheet), "the sheet is open");
        for dt in [0.0, 50.0, 100.0, 250.0] {
            rig.shot(6000.0 + dt, &format!("CloseSheet+{dt}"));
        }
    }
}

/// C1 level 27 (slot 26), as the dashed-beam report has it: its stored solution with the two
/// dopplers moved to the middle row of the outer columns, so light goes up a column and comes
/// back down, and its second and third turns of the right one light halves both ways (main
/// 45fa3de). Their sweep frames, at the bench's desktop and through `Rgba` at the web's size:
/// with `strands` giving the first strand alone (the dashed sweep), 24 of these 32 lines change.
fn both_ways(theme: Theme) {
    let (game, slot) = (Game::C1, 26);
    let run = Run { theme, game, slot };
    for (spec, rgba) in [(EASELS[0], false), (EASELS[2], true)] {
        let mut rig = Rig::new(level(game, slot), spec, rgba, run);
        rig.start();
        let tableau = rig.lb.tableau().clone();
        let at = |x, y| centre_of(&tableau, board_index(x, y));
        for (from, to) in [(at(0, 8), at(0, 6)), (at(3, 14), at(14, 6))] {
            rig.input(InputEvent::MouseDown {
                x: from.0,
                y: from.1,
                alt: false,
            });
            rig.input(InputEvent::MouseDragged { x: to.0, y: to.1 });
            rig.input(InputEvent::MouseUp {
                x: to.0,
                y: to.1,
                alt: false,
            });
        }
        rig.lb.paint(&rig.state, 1000.0, rig.buf.out());
        let (x, y) = at(14, 6);
        // The first turn lights no half both ways; the next two do, mid-sweep.
        for k in 0..3 {
            rig.input(InputEvent::MouseDown { x, y, alt: false });
            rig.input(InputEvent::MouseUp { x, y, alt: false });
            let t = 4000.0 + 4000.0 * f64::from(k);
            if k == 0 {
                rig.lb.paint(&rig.state, t + 3000.0, rig.buf.out());
                continue;
            }
            for dt in [50.0, 100.0, 150.0, 250.0] {
                rig.shot(t + dt, &format!("bothways{k}+{dt}"));
            }
        }
    }
}

#[test]
#[ignore]
fn print_the_paint_ledger() {
    for theme in Theme::ALL {
        for game in Game::ALL {
            for slot in slots(game) {
                let run = Run { theme, game, slot };
                let state = level(game, slot);
                let mut lightbox = Lightbox::new(easel(EASELS[0]), theme, false);
                for spec in EASELS {
                    let e = easel(spec);
                    lightbox.resize(e);
                    let mut out = vec![0u32; (e.width * e.height) as usize];
                    lightbox.paint(&state, SETTLED, Output::Xrgb(&mut out));
                    line("settled", run, e, "-", hash(&out));
                }
                for spec in [EASELS[2], EASELS[7]] {
                    let e = easel(spec);
                    lightbox.resize(e);
                    let mut bytes = vec![0u8; (4 * e.width * e.height) as usize];
                    lightbox.paint(&state, SETTLED, Output::Rgba(&mut bytes));
                    let h = fnv(bytes.iter().copied());
                    line("rgba", run, e, "-", h);
                }
            }
            cues(theme, game);
        }
        both_ways(theme);
    }
}
