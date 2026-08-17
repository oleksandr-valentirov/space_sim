//! The game's window (ROADMAP J1).
//!
//! The event loop here is its own rather than borrowed from `engine::app`, and
//! that is a boundary rather than duplication: the game owns the world and
//! time (PROJECT.md §6), the engine does not. What stays shared is everything
//! where one can err silently: the surface and its states live in
//! `engine::window::Target`, the frame in `engine::frame::Frame`, the camera
//! in `engine::orbit`.
//!
//! The frame's order, final since J4:
//!
//!   1. take an immutable slice -- `Sim::snapshot`, exactly once;
//!   2. collect the events accumulated in the channel;
//!   3. translate the slice into a scene;
//!   4. draw.
//!
//! The main thread neither computes nor touches the world -- it **reads** it.
//! Everything the player does with time and the plan goes as a command into
//! the simulation thread (`crate::sim`), and the answer arrives with the next
//! snapshot.

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::ui::{Ui, Viewport, WindowInput};
use engine::window::{self, Target};

use crate::clock::Stall;
use crate::frame_view::ViewFrame;
use crate::hud;
use crate::leg::restart_at;
use crate::mission;
use crate::node;
use crate::plan::Manoeuvre;
use crate::planner::{Planner, Preview, PreviewRequest, Request};
use crate::porkchop::{Grid, GridRequest};
use crate::save::{self, Save};
use crate::schedule;
use crate::sim::{Command, Event, Sim};
use crate::text::{tr, Key as TextKey, Language};
use crate::view;
use crate::world::{PlanRejected, World, EARTH, MOON};

pub struct Options {
    pub width: u32,
    pub height: u32,
    /// How many frames to draw before exiting. `None` means until closed.
    pub frames: Option<u32>,
    pub vsync: bool,
    pub asset: std::path::PathBuf,
    /// Add the demonstration manoeuvre (`mission::demo_plan`).
    pub demo_plan: bool,
    /// Raise the game from a save instead of a new mission.
    pub load: Option<std::path::PathBuf>,
    /// How many stations to add to the mission (`mission::fleet`). Zero means
    /// the mission alone.
    ///
    /// The N1 measurement fixture: the trail hits its ceiling from the number
    /// of vessels rather than from years, so this number is what sets up a
    /// scene where debt D7 is visible.
    pub stations: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 1280,
            height: 720,
            frames: None,
            vsync: true,
            asset: mission::default_asset(),
            demo_plan: false,
            load: None,
            stations: 0,
        }
    }
}

struct App {
    options: Options,
    drawn: u32,
    state: Option<State>,
    error: Option<String>,
}

struct State {
    target: Target,
    gpu: Gpu,
    frame: Frame,
    orbit: Orbit,

    /// The interface: the plumbing in the engine, the widgets here (U1b).
    ui: Ui,
    input: WindowInput,
    language: Language,
    /// Earth's radius from the asset, read **once** (`eph_body_radius`, U2a).
    /// The altitude panel does not recompute it every frame: a body's size
    /// does not change, and rule 5 forbids calling the ephemeris from a
    /// frame.
    earth_radius_m: f64,

    /// The per-leg thinned trail (N2b). Lives here because it is **view**
    /// state: no number in the world depends on it, and the simulation thread
    /// does not know about it.
    trails: crate::trail::Cache,

    /// The Moon's terrain, loaded into the frame at startup (D12).
    ///
    /// An `Option`, and not out of caution: `Frame::load_terrain` legitimately
    /// refuses where the adapter gave no bindless, and the asset may simply be
    /// absent (`make cook-dem` makes it, and it is not in git). A game with a
    /// smooth Moon is a working state, not an error.
    moon_terrain: Option<engine::scene::TerrainId>,
    /// The same for Earth (T7g). The second body with tiles, and the one that
    /// took debt D19 over its own threshold.
    earth_terrain: Option<engine::scene::TerrainId>,

    /// The world lives in its own thread; here there is only a handle to it
    /// (`crate::sim`). The main thread neither computes nor touches the world
    /// -- it reads it.
    sim: Sim,

    /// Speculative runs (`crate::planner`). They write nothing into the world
    /// until the player says so.
    planner: Planner,
    preview: Option<Preview>,
    next_request: u64,

    dragging: bool,
    cursor: Option<(f64, f64)>,

    /// The plan the player is editing (ROADMAP-UI.md, U4a). UI-owned state: it
    /// does not exist outside the screen until it goes as a request or a
    /// commit.
    draft: hud::PlanDraft,
    /// The world's last answer about the plan -- what the panel shows instead
    /// of its own assumption of success (rule 8).
    notice: Option<String>,

    /// The draft's nodes on screen, computed by the previous frame (U4b).
    ///
    /// The previous one specifically: a mouse event arrives between frames,
    /// while the nodes depend on the camera and on what is already computed. A
    /// frame is the moment the player saw them.
    nodes: Vec<node::NodeOnScreen>,
    /// The grabbed handle, while the button is held.
    grab: Option<node::Grab>,
    /// The draft changed by dragging -- a preview must be requested.
    draft_changed: bool,

    /// The transfer-window plot: texture and selected window (U5c).
    plot: hud::PlotState,
    /// The last grid from the planner. Not world state -- an answer to a
    /// request.
    grid: Option<Grid>,
    /// Earth's `mu` from the asset, read **once**, like the radius above.
    earth_mu: f64,

    /// Which frame to show the scene in (ROADMAP-UI.md, U6a4).
    ///
    /// View state, not world state: it does not travel into the world thread
    /// and changes no number in the snapshot. Hence it lives here rather than
    /// in `sim`.
    view_frame: ViewFrame,

    /// The body the camera turns around (the second half of D12).
    ///
    /// A body id rather than an index into `snapshot.bodies`: the snapshot is
    /// rebuilt every frame, and an index would silently mean a different body
    /// the moment the asset's body list changed. View state again -- the world
    /// does not know where anyone is looking.
    camera_target: i32,
}

/// The Moon's cooked terrain, from the repository root.
///
/// The same file the engine's demo reads (`engine::demo::TERRAIN_ASSET`), but
/// the path is repeated here rather than borrowed: the demo is the engine's
/// fixture, and a game taking a constant from it would tie itself to the
/// engine's debugging tool.
pub const MOON_TERRAIN_ASSET: &str = "assets/moon.dem";

/// The Moon's cooked colour (stage T, T2d). A separate file from the terrain,
/// for the same reason the format is separate: pyramids of different depth
/// (T2c).
pub const MOON_COLOUR_ASSET: &str = "assets/moon.col";

/// Earth's cooked terrain and colour (stage T, T7d and T7e).
///
/// The second body with tiles, and the one that made the cost of bindless
/// arrays worth paying down: binding was charged by the array's length rather
/// than by what was drawn, which is what debt D19 was. Closed by Y1 -- the
/// frame now binds the tiles it reads.
pub const EARTH_TERRAIN_ASSET: &str = "assets/earth.dem";
pub const EARTH_COLOUR_ASSET: &str = "assets/earth.col";

/// The cooked star catalogue (stage Z, Z2). `make cook-stars` makes it, and it
/// is not in git either.
pub const STAR_CATALOGUE_ASSET: &str = "assets/stars.cat";

/// Loads the star catalogue into the frame (stage Z).
///
/// **Takes the path rather than reading the constant**, unlike the terrain
/// loaders above, and the reason is the oracle: the cooked catalogue is not in
/// git, so a test that could only read `assets/stars.cat` would pass by
/// skipping on every machine that has not cooked one -- which includes CI.
/// With the path as an argument the game's own loading path is checked against
/// a catalogue the test writes itself.
///
/// **Says loudly when it did not work, and carries on**, for the same reason
/// as the terrain: a sky without stars is the state the game was in until this
/// step, and it is a working state. A *silent* one would be what debt D12 was
/// -- an asset that supposedly exists while the screen shows nothing.
pub fn load_stars(gpu: &Gpu, frame: &mut Frame, path: &std::path::Path) {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!(
                "no star catalogue ({}: {e}) -- the sky stays black.",
                path.display()
            );
            eprintln!("to fix: cargo run -p star-cook");
            return;
        }
    };

    match engine::stars::Catalogue::from_bytes(&bytes) {
        Ok(catalogue) => {
            let count = catalogue.stars.len();
            frame.load_stars(gpu, &catalogue);
            println!("stars: {}, {count} of them", path.display());
        }
        Err(e) => {
            eprintln!(
                "the star catalogue {} does not read ({e}) -- the sky stays black.",
                path.display()
            );
        }
    }
}

/// Loads the Moon's terrain into the frame (D12).
///
/// **Says loudly when it did not work, and carries on.** Three reasons for
/// having no terrain are legitimate: the asset is missing (it is not in git,
/// `make cook-dem` makes it), an adapter without bindless, a corrupt file.
/// None of them makes the game unusable -- it draws a smooth Moon, as it did
/// before this step. A **silent** smooth Moon, though, would be exactly what
/// D12 was: terrain that supposedly exists while the screen shows none, and
/// nobody knows why.
pub fn load_moon_terrain(gpu: &Gpu, frame: &mut Frame) -> Option<engine::scene::TerrainId> {
    load_surface(
        gpu,
        frame,
        "the Moon",
        MOON_TERRAIN_ASSET,
        MOON_COLOUR_ASSET,
    )
}

/// Loads Earth's surface into the frame (T7g) -- by the same path as the
/// Moon's.
///
/// No separate function is introduced: only two paths and a word in the report
/// differ, and everything else is the same absence, the same leniency and the
/// same handle. Two copies of this code would diverge at the first edit.
pub fn load_earth_terrain(gpu: &Gpu, frame: &mut Frame) -> Option<engine::scene::TerrainId> {
    load_surface(gpu, frame, "Earth", EARTH_TERRAIN_ASSET, EARTH_COLOUR_ASSET)
}

fn load_surface(
    gpu: &Gpu,
    frame: &mut Frame,
    whose: &str,
    terrain_asset: &str,
    colour_asset: &str,
) -> Option<engine::scene::TerrainId> {
    let bytes = match std::fs::read(terrain_asset) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("no terrain for {whose} ({terrain_asset}: {e}) -- drawing smooth.");
            eprintln!("to fix: make cook-dem");
            return None;
        }
    };

    let terrain = match engine::tiles::Terrain::from_bytes(&bytes) {
        Ok(terrain) => terrain,
        Err(e) => {
            eprintln!("terrain {terrain_asset} does not read ({e}) -- drawing smooth.");
            return None;
        }
    };

    // Colour is a separate asset and a separate absence (T2c). Without it the
    // Moon stays grey per `Body::colour`, exactly as before stage T; the
    // mountains go nowhere meanwhile.
    let colour = match std::fs::read(colour_asset) {
        Ok(bytes) => match engine::tiles::Colour::from_bytes(&bytes) {
            Ok(colour) => Some(colour),
            Err(e) => {
                eprintln!("colour {colour_asset} does not read ({e}) -- drawing grey.");
                None
            }
        },
        Err(e) => {
            eprintln!("no colour for {whose} ({colour_asset}: {e}) -- drawing grey.");
            eprintln!("to fix: make cook-colour");
            None
        }
    };

    let levels = terrain.levels;
    let colour_levels = colour.as_ref().map(|c| c.levels);
    match frame.load_surface(gpu, &terrain, colour.as_ref()) {
        Ok(id) => {
            println!("terrain for {whose}: {terrain_asset}, {levels} pyramid levels");
            if let Some(levels) = colour_levels {
                println!("colour for {whose}: {colour_asset}, {levels} pyramid levels");
            }
            Some(id)
        }
        Err(e) => {
            eprintln!("the surface did not load into the frame ({e}) -- drawing smooth.");
            None
        }
    }
}

/// The world per the options -- shared by the window and the capture.
pub fn build_world(options: &Options) -> Result<World, String> {
    if let Some(path) = &options.load {
        // The save needs the ephemeris ready: it carries state and plan but
        // not the asset (`crate::save`).
        let eph = core_rs::Ephemeris::load(&options.asset)
            .map(std::sync::Arc::new)
            .map_err(|e| format!("the asset does not read ({}): {e}", options.asset.display()))?;

        return Save::read(path)?
            .into_world(eph, mission::config())
            .map_err(|e| format!("the save does not load ({}): {e}", path.display()));
    }

    // The fleet outranks `--demo-plan`: the showcase manoeuvre belongs to the
    // halo orbit while the measurement fixture asks about the number of
    // vessels. Nobody needs them together, and silently combining them would
    // mean measuring a third scene.
    if options.stations > 0 {
        return mission::fleet(&options.asset, options.stations).map_err(|e| {
            format!(
                "the fleet does not build ({}): {e}",
                options.asset.display()
            )
        });
    }

    let build = if options.demo_plan {
        mission::world_with_demo_plan
    } else {
        mission::world
    };
    build(&options.asset).map_err(|e| {
        format!(
            "the world does not build ({}): {e}",
            options.asset.display()
        )
    })
}

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("no event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        options,
        drawn: 0,
        state: None,
        error: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("the event loop stopped with an error: {e}"))?;

    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        match State::new(event_loop, &self.options) {
            Ok(state) => {
                println!("adapter: {}", state.gpu.describe());
                println!("surface: {}", state.target.describe());
                println!("asset: {}", self.options.asset.display());
                println!(
                    "camera: drag with the left button to rotate, wheel for altitude,\n\
                     \x20       Tab moves to the next body\n\
                     time: space pauses, '.' and ',' double warp\n\
                     plan: 'p' shows a braking burn in 5 days, Enter flies it\n\
                     F5 saves, Esc exits"
                );
                state.report_time(&state.sim.snapshot());
                self.state = Some(state);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Input has an owner, and that is asked **exactly here**, before
        // anything else (ROADMAP-UI.md, rule 4 and U1c). `consumed` means "the
        // game does not see this event": clicking a button must not rotate the
        // camera as well.
        let consumed = state.input.on_window_event(state.target.window(), &event);

        match event {
            // Close, resize and redraw belong to nobody: egui sees them too,
            // but the window must answer them regardless.
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() || consumed {
                    return;
                }
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    // Time controls are commands into the thread rather than
                    // calls. The answer arrives with the next snapshot.
                    Key::Named(NamedKey::Space) => state.sim.send(Command::TogglePause),
                    // Warp multiplies rather than adds: 1 to 1e7 is seven
                    // decades (`crate::clock`).
                    Key::Character(".") => state.sim.send(Command::ScaleWarp(2.0)),
                    Key::Character(",") => state.sim.send(Command::ScaleWarp(0.5)),
                    // Show what happens if a braking burn comes in five
                    // days.
                    Key::Character("p") => state.ask_for_preview(),
                    // And fly that plan.
                    Key::Named(NamedKey::Enter) => state.commit_preview(),
                    Key::Named(NamedKey::F5) => {
                        state.sim.send(Command::Save(save::default_path()));
                    }
                    // What the camera turns around (D12). Reads the snapshot
                    // here rather than deferring to the frame: the choice is
                    // over the bodies that exist, and the snapshot is where
                    // they are said to exist.
                    //
                    // This arm was unreachable until X1: egui reports Tab as
                    // consumed whether or not it wants the keyboard, so
                    // `consumed` above was always true for it. The exception
                    // lives in `engine::ui::tab_is_the_games`, with the rest of
                    // the ownership rule, and not here.
                    Key::Named(NamedKey::Tab) => {
                        let snapshot = state.sim.snapshot();
                        state.cycle_camera_target(&snapshot);
                    }
                    _ => {}
                }
            }

            WindowEvent::Resized(size) => state.target.resize(&state.gpu, size.width, size.height),

            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                // A press that started in a panel does not belong to the world
                // -- otherwise the pause button would start rotating the
                // camera as well.
                let pressed = button_state == ElementState::Pressed && !consumed;

                // A node handle first and only then the camera: dragging a
                // handle is editing the plan rather than looking at it
                // (U4b).
                state.grab = None;
                if pressed {
                    if let Some(cursor) = state.cursor {
                        let at = [cursor.0 as f32, cursor.1 as f32];
                        state.grab = node::pick_handle(&state.nodes, at);
                    }
                }

                state.dragging = pressed && state.grab.is_none();
                if button_state != ElementState::Pressed {
                    state.cursor = None;
                }
            }

            WindowEvent::CursorLeft { .. } => state.cursor = None,

            WindowEvent::CursorMoved { position, .. } => {
                let now = (position.x, position.y);
                if let (Some(grab), Some(was)) = (state.grab, state.cursor) {
                    // Dragging a handle edits exactly one dv component of the
                    // grabbed axis -- the rest do not move by construction
                    // (U4b).
                    let drag = [(now.0 - was.0) as f32, (now.1 - was.1) as f32];
                    if let Some(node) = state.nodes.iter().find(|n| n.index == grab.node) {
                        let delta = node::drag_to_delta(node, grab.axis, drag);
                        if let Some(manoeuvre) = state.draft.manoeuvres.get_mut(grab.node) {
                            manoeuvre.dv[grab.axis] += delta;
                            state.draft_changed = true;
                        }
                    }
                } else if state.dragging {
                    if let Some(was) = state.cursor {
                        state.orbit.drag(now.0 - was.0, now.1 - was.1);
                    }
                }
                state.cursor = Some(now);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if consumed {
                    return;
                }
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / 50.0,
                };
                state.orbit.zoom(notches);
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = state.draw() {
                    self.error = Some(e);
                    event_loop.exit();
                    return;
                }

                self.drawn += 1;

                if let Some(limit) = self.options.frames {
                    if self.drawn >= limit {
                        let snapshot = state.sim.snapshot();
                        println!(
                            "frames drawn: {}, prediction samples: {}",
                            self.drawn,
                            snapshot
                                .vessels
                                .iter()
                                .map(|v| v.sample_count())
                                .sum::<usize>()
                        );
                        state.report_time(&snapshot);
                        event_loop.exit();
                        return;
                    }
                }

                state.target.window().request_redraw();
            }

            _ => {}
        }
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<State, String> {
        let (target, gpu) = Target::open(
            event_loop,
            &window::Options {
                title: "space_sim".to_string(),
                width: options.width,
                height: options.height,
                vsync: options.vsync,
            },
        )?;

        let mut frame = Frame::new(&gpu, target.format());
        let moon_terrain = load_moon_terrain(&gpu, &mut frame);
        let earth_terrain = load_earth_terrain(&gpu, &mut frame);
        load_stars(&gpu, &mut frame, std::path::Path::new(STAR_CATALOGUE_ASSET));
        let ui = Ui::new(&gpu, target.format());
        // The instrument palette (U7c) once at startup rather than every
        // frame: the `Style` inside the context lives on by itself, and
        // reinstalling it in a frame would mean no widget could change the
        // style temporarily.
        //
        // The theme is nailed to dark, and the same style is installed for
        // **both**: this game has no light theme -- an instrument panel gone
        // white from a system setting is a broken frame rather than a styling
        // variant. One `set_theme` is not enough for that: it says which theme
        // to take, not what is in it.
        crate::palette::apply(ui.context());
        let input = WindowInput::new(&ui, target.window());
        let sim = Sim::spawn(build_world(options)?)?;
        let earth_radius_m = sim.ephemeris().body_radius(EARTH);
        let earth_mu = sim.ephemeris().body_mu(EARTH);
        // The planner shares the asset with the simulation but not the
        // propagator: `Ephemeris` is `Sync`, `Propagator` is not (D3, H4).
        let planner = Planner::spawn(sim.ephemeris(), mission::config())?;

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            trails: crate::trail::Cache::new(),
            ui,
            input,
            // English as the primary language (ROADMAP-UI.md, rule 7); the
            // toggle is in the view panel (U7a).
            language: Language::default(),
            earth_radius_m,
            moon_terrain,
            earth_terrain,
            orbit: Orbit::at_altitude(mission::CAMERA_ALTITUDE_M),
            sim,
            planner,
            preview: None,
            next_request: 0,
            dragging: false,
            cursor: None,
            draft: hud::PlanDraft::default(),
            notice: None,
            nodes: Vec::new(),
            grab: None,
            draft_changed: false,
            plot: hud::PlotState::default(),
            view_frame: ViewFrame::default(),
            grid: None,
            earth_mu,
            camera_target: EARTH,
        })
    }

    /// The camera for this frame, turning around whatever is targeted.
    ///
    /// Rebuilt per frame rather than cached, because the target moves: the
    /// Moon covers its own diameter in about an hour of game time, and at warp
    /// that is a couple of frames. A centre captured once would leave the
    /// camera aiming at where the Moon used to be.
    ///
    /// Falls back to the origin where the target is not in the snapshot. That
    /// is Earth, i.e. exactly the view the game had before this step -- the
    /// same leniency as the terrain and the stars, and for the same reason:
    /// a body missing from the asset must not be a dead window.
    fn camera(&self, snapshot: &crate::snapshot::WorldSnapshot) -> engine::camera::Camera {
        let centre =
            view::body_centre(snapshot, self.camera_target, self.view_frame).unwrap_or([0.0; 3]);
        self.orbit.camera_about(centre)
    }

    /// Moves the camera to the next body in the snapshot that can be drawn.
    ///
    /// "Can be drawn" is the same test `view` applies -- a positive radius --
    /// so the cycle cannot stop on something invisible. The order is the
    /// asset's, which is stable within a run, and that is all the player needs
    /// from it: a key that always advances and eventually comes back.
    fn cycle_camera_target(&mut self, snapshot: &crate::snapshot::WorldSnapshot) {
        let drawable: Vec<_> = snapshot
            .bodies
            .iter()
            .filter(|b| b.radius_m > 0.0)
            .collect();
        if drawable.is_empty() {
            return;
        }

        let at = drawable
            .iter()
            .position(|b| b.body == self.camera_target)
            .map_or(0, |i| (i + 1) % drawable.len());
        let chosen = drawable[at];

        self.camera_target = chosen.body;
        // The altitude is a statement about the body under it, so the wheel
        // must mean the same thing after the switch as before it.
        self.orbit.set_reference(chosen.radius_m);
        println!(
            "camera target: body {} (radius {:.0} km)",
            chosen.body,
            chosen.radius_m / 1000.0
        );
    }

    /// Prints the clock's state from the snapshot -- **for runs without eyes**.
    ///
    /// Since U2b the time panel (`crate::hud`) shows the same, and it is now
    /// the main one. This print stays for `--frames N`, which exits by itself
    /// and shows nobody anything: a line in stdout is all that can be read out
    /// of such a run in CI.
    fn report_time(&self, snapshot: &crate::snapshot::WorldSnapshot) {
        let day = (snapshot.t - mission::start().t) / 86400.0;
        println!(
            "  day {day:.2} of {:.2}, warp x{:.0}{}",
            mission::DAYS,
            snapshot.warp,
            match snapshot.stall {
                Some(Stall::Paused) => " (paused)",
                Some(Stall::Horizon) => " (hitting the horizon)",
                Some(Stall::MissionEnd) => " (mission over)",
                None => "",
            }
        );
    }

    /// Asks for a prediction of a hypothetical braking burn in five days.
    ///
    /// One button instead of dragging a node: the flight planner's UI is M3,
    /// and what is checked here is the path rather than the interface.
    fn ask_for_preview(&mut self) {
        let snapshot = self.sim.snapshot();
        let Some(vessel) = snapshot.vessels.first() else {
            return;
        };

        let burn_t = snapshot.t + 5.0 * 86400.0;
        if burn_t >= vessel.horizon_end || vessel.computed_to < burn_t {
            println!("preview: the prediction has not reached that day yet");
            return;
        }

        let mut plan = vessel.plan.clone();
        plan.insert(Manoeuvre {
            t: burn_t,
            dv: [-8.0, 0.0, 0.0],
            frame: crate::plan::Frame::Vnb { body: EARTH },
        });

        // The restart point comes from the same function the simulation will
        // use when it accepts the plan. One function rather than two identical
        // rules.
        let restart = restart_at(&vessel.legs, vessel.start, burn_t);

        self.next_request += 1;
        self.planner.request(Request::Preview(PreviewRequest {
            id: self.next_request,
            vessel: vessel.id,
            from: restart.state,
            step: restart.step,
            plan,
            horizon_end: vessel.horizon_end,
            params: vessel.params,
        }));
    }

    /// What to do with what the plan panel asked for (ROADMAP-UI.md, U4a).
    ///
    /// A preview goes to the planner, a commit into the simulation thread. The
    /// split is not cosmetic: the planner writes nothing into the world (J5),
    /// which is exactly why an edit can be shown on every movement of a
    /// slider.
    fn apply_plan_action(
        &mut self,
        action: hud::PlanAction,
        snapshot: &crate::snapshot::WorldSnapshot,
    ) {
        let Some(vessel) = snapshot.vessels.first() else {
            return;
        };

        match action {
            hud::PlanAction::Preview(plan) => {
                // The restart point comes from the same function the
                // simulation will use when it accepts the plan. One function
                // rather than two identical rules.
                let Some(first) = plan.manoeuvres().first() else {
                    // An empty plan needs no preview: flying it is the same as
                    // what is already drawn.
                    self.preview = None;
                    return;
                };
                let restart = restart_at(&vessel.legs, vessel.start, first.t);

                self.next_request += 1;
                self.planner.request(Request::Preview(PreviewRequest {
                    id: self.next_request,
                    vessel: vessel.id,
                    from: restart.state,
                    step: restart.step,
                    plan,
                    horizon_end: vessel.horizon_end,
                    params: vessel.params,
                }));
            }
            hud::PlanAction::Commit(plan) => {
                self.notice = None;
                self.sim.send(Command::CommitPlan {
                    vessel: vessel.id,
                    plan,
                });
            }
        }
    }

    /// What to do with what the window plot asked for (U5c).
    fn apply_porkchop_action(
        &mut self,
        action: hud::PorkchopAction,
        snapshot: &crate::snapshot::WorldSnapshot,
    ) {
        match action {
            hud::PorkchopAction::Compute => {
                let Some(vessel) = snapshot.vessels.first() else {
                    return;
                };
                self.next_request += 1;
                if let Some(request) = self.grid_request(snapshot.t, vessel) {
                    self.planner.request(Request::Grid(request));
                } else {
                    self.notice = Some(tr(self.language, TextKey::NoGridYet).to_string());
                }
            }
            hud::PorkchopAction::Choose(i, j) => self.choose_window(i, j, snapshot),
        }
    }

    /// The selected window becomes a manoeuvre in the plan draft (U5d).
    ///
    /// Nothing is recomputed, and that is the point: the cell already carries
    /// `dv` -- the same impulse it was found with (`porkchop::Cell`). A second
    /// Lambert solution here would mean two numbers that **must** agree and
    /// therefore will one day diverge; besides, it would need the ephemeris in
    /// the frame, which rule 5 does not allow.
    ///
    /// The manoeuvre goes in the inertial frame, because that is the frame the
    /// cell was computed in. Translating it into VNB would mean rewriting
    /// something already exact through a basis that still has to be
    /// checked.
    fn choose_window(&mut self, i: usize, j: usize, snapshot: &crate::snapshot::WorldSnapshot) {
        let Some(grid) = self.grid.as_ref() else {
            return;
        };
        let (Some(&t1), Some(cell)) = (grid.t1.get(i), grid.at(i, j)) else {
            // A hole is not a choice. The panel already said so in words, and
            // the silent absence of a manoeuvre here agrees with what it
            // showed.
            self.notice = Some(tr(self.language, TextKey::NoSolution).to_string());
            return;
        };

        // The world rejects a manoeuvre in the past
        // (`PlanRejected::InThePast`), and it is better to say so before the
        // refusal than after.
        if t1 <= snapshot.t {
            self.notice = Some(tr(self.language, TextKey::RejectedInThePast).to_string());
            return;
        }

        // A replacement rather than an addition: the plot is the choice of
        // **one** window, and a second click means "not that, this". Piling up
        // manoeuvres is the plan editor's job, where they are visible as a
        // list (U4a).
        self.draft.manoeuvres.retain(|m| m.t != t1);
        self.draft.manoeuvres.push(Manoeuvre {
            t: t1,
            dv: cell.dv,
            frame: crate::plan::Frame::Inertial,
        });
        self.notice = None;

        // And immediately show what **really** comes of it: a cell is a
        // Keplerian two-body estimate while the preview is computed with the
        // full force model. The difference between them is visible on screen,
        // and that is honest: the grid chooses a window rather than promising
        // a trajectory.
        self.apply_plan_action(hud::PlanAction::Preview(self.draft.plan()), snapshot);
    }

    /// The grid's axes: departure instants from the trajectory, flight times
    /// from half a day.
    ///
    /// Departure goes **no further than what is computed**, and that is a
    /// condition of correctness rather than caution: past the horizon
    /// `leg::state_at` returns the endpoint, so a grid built on it would show
    /// a transfer from a place the vessel will not be. `None` means "the
    /// prediction has not left the cursor yet" -- then there is nothing to
    /// compute from, and saying so is more honest than drawing a row of
    /// nothing but holes.
    ///
    /// The remaining numbers are a property of this mission rather than of the
    /// plot: an Earth-Moon transfer takes three to seven days, so an axis from
    /// half a day to fourteen leaves both edges of the forbidden zones
    /// visible.
    fn grid_request(
        &self,
        now: f64,
        vessel: &crate::snapshot::VesselSnapshot,
    ) -> Option<GridRequest> {
        const DAY: f64 = 86400.0;
        const COLUMNS: usize = 40;

        // The departure axis's step: exactly enough for forty columns to fit
        // inside what is computed. An empty horizon is the `None` above.
        let span = vessel.computed_to - now;
        if span < DAY {
            return None;
        }
        let step = (span / COLUMNS as f64).min(DAY);

        let depart = (0..COLUMNS)
            .map(|i| crate::leg::state_at(&vessel.legs, vessel.start, now + i as f64 * step))
            .collect();

        Some(GridRequest {
            id: self.next_request,
            depart,
            arrive_body: MOON,
            centre_body: EARTH,
            mu: self.earth_mu,
            prograde: true,
            tof: (1..=28).map(|j| f64::from(j) * 0.5 * DAY).collect(),
        })
    }

    /// Fly the plan as shown.
    fn commit_preview(&mut self) {
        let Some(preview) = self.preview.take() else {
            println!("nothing to commit -- press 'p' first");
            return;
        };
        self.sim.send(Command::CommitPlan {
            vessel: preview.vessel,
            plan: preview.plan,
        });
    }

    fn draw(&mut self) -> Result<(), String> {
        // Exactly once per frame, held for the whole frame: two loads would
        // give two different instants in one picture.
        let snapshot = self.sim.snapshot();

        // The preview from the planner: take the freshest, discard the
        // older.
        if let Some(preview) = self.planner.latest() {
            println!("preview {}: {} legs", preview.id, preview.legs.len());
            self.preview = Some(preview);
        }

        // The window grid likewise -- a different channel, the same rule
        // (U5b).
        if let Some(grid) = self.planner.latest_grid() {
            println!(
                "grid {}: {} cells, {} without a solution",
                grid.id,
                grid.cells.len(),
                grid.cells.iter().filter(|c| c.is_none()).count()
            );
            // A selection from the previous grid does not carry over to the
            // new one: the indices are the same while the axes differ, so "the
            // same window" would point at a different time.
            self.plot.chosen = None;
            self.grid = Some(grid);
        }

        // Discrete things arrive by channel rather than by snapshot
        // (`crate::sim`).
        for event in self.sim.events() {
            match event {
                Event::VesselFailed { vessel, error } => {
                    println!("vessel {vessel:?} stopped: {error}");
                }
                // Rule 8 of stage U: the panel shows the answer rather than
                // its own assumption of success. Until there is a message
                // panel, stdout, like the rest of the events here.
                Event::SeekRejected { t, why } => {
                    println!("seek to {t:.1} rejected: {why:?}");
                }
                Event::PlanRejected { vessel, why } => {
                    println!("plan for {vessel:?} rejected: {why:?}");
                    self.notice = Some(
                        match why {
                            PlanRejected::InThePast => {
                                tr(self.language, TextKey::RejectedInThePast)
                            }
                            PlanRejected::NoSuchVessel => tr(self.language, TextKey::Failed),
                        }
                        .to_string(),
                    );
                }
                Event::Saved { error } => match error {
                    Some(e) => println!("the save was not written: {e}"),
                    None => println!("save: {}", save::default_path().display()),
                },
                Event::PlanCommitted { vessel, from } => {
                    println!("plan for {vessel:?} accepted, recomputing from {from:?}");
                    self.notice = Some(tr(self.language, TextKey::PlanAccepted).to_string());
                    // The preview became reality -- erase it from the frame.
                    self.preview = None;
                }
            }
        }

        let Some(surface) = self.target.acquire(&self.gpu)? else {
            return Ok(());
        };

        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("game frame"),
            });

        // The thinned trail rather than the full one: N1 measured 23.7 ms per
        // frame with thirty vessels, N2b brought that to 11.8. The frame
        // height is the one currently drawn into, because the criterion is on
        // screen.
        // One camera for the whole frame: the scene and the node handles must
        // project through the same one, or a dragged handle would sit beside
        // the trajectory it belongs to. Taken before `thinning` borrows
        // `self.trails` -- it reads `self`, and the borrow checker is right
        // that the two cannot overlap.
        let camera = self.camera(&snapshot);
        let mut thinning = view::Thinning {
            cache: &mut self.trails,
            height_px: self.target.height(),
        };
        let mut scene = view::build_thinned(
            &snapshot,
            camera,
            self.preview.as_ref().map_or(&[], |p| p.legs.as_slice()),
            self.view_frame,
            &mut thinning,
        );
        // Terrain after the scene is built and only if it loaded (D12).
        // `view` knows nothing of the frame, and it is the frame that issues
        // the handle.
        if let Some(terrain) = self.moon_terrain {
            view::attach_terrain(&mut scene, &snapshot, MOON, terrain);
        }
        if let Some(terrain) = self.earth_terrain {
            view::attach_terrain(&mut scene, &snapshot, EARTH, terrain);
        }

        self.frame.draw(
            &self.gpu,
            &mut encoder,
            &view,
            self.target.width(),
            self.target.height(),
            &scene,
        );

        // The interface as the last pass, into the same texture (U1b). A panel
        // returns commands rather than sending them: whoever sends knows about
        // the channel, while a panel knows only what was drawn.
        let mut commands = Vec::new();
        let mut plan_actions = Vec::new();
        // The panels' closure borrows `self` only through copies, so the frame
        // choice comes back here and is applied after the frame -- like the
        // commands beside it.
        let view_frame = self.view_frame;
        let curve = hud::read_curve(&snapshot);
        let mut view_choice = hud::ViewChoice::default();
        let mut plot_actions = Vec::new();
        let language = self.language;
        let radius = self.earth_radius_m;
        let notice = self.notice.clone();
        let draft = &mut self.draft;
        let plot = &mut self.plot;
        let grid = self.grid.as_ref();
        let viewport = Viewport::new(
            self.target.width(),
            self.target.height(),
            self.target.window().scale_factor() as f32,
        );
        let input = self.input.take(self.target.window());
        let platform = self
            .ui
            .draw(&self.gpu, &mut encoder, &view, viewport, input, |ui| {
                engine::egui::Panel::left("time")
                    .exact_size(220.0)
                    .resizable(false)
                    .show(ui, |ui| {
                        commands.extend(hud::time_panel(ui, language, &snapshot));
                        ui.separator();
                        if let Some(vessel) = snapshot.vessels.first() {
                            let readout = hud::read_vessel(&snapshot, vessel, radius);
                            hud::vessel_panel(ui, language, &vessel.name, &readout);

                            ui.separator();
                            // A scan over already computed samples -- nothing
                            // is armed and nothing is integrated (U3a).
                            let markers = schedule::scan(&vessel.legs);
                            commands
                                .extend(hud::schedule_panel(ui, language, snapshot.t, &markers));

                            ui.separator();
                            plan_actions.extend(hud::plan_panel(
                                ui,
                                language,
                                snapshot.t,
                                EARTH,
                                draft,
                                notice.as_deref(),
                            ));
                        }

                        ui.separator();
                        view_choice = hud::view_panel(ui, language, view_frame, curve);
                    });

                // The plot goes on the right in its own panel: it is square
                // and lives its own life, while the left column is already
                // about the vessel and its plan.
                engine::egui::Panel::right("windows")
                    .exact_size(230.0)
                    .resizable(false)
                    .show(ui, |ui| {
                        plot_actions.extend(hud::porkchop_panel(ui, language, grid, plot));
                    });
            });
        self.input.apply(self.target.window(), platform);

        for command in commands {
            self.sim.send(command);
        }
        for action in plan_actions {
            self.apply_plan_action(action, &snapshot);
        }
        for action in plot_actions {
            self.apply_porkchop_action(action, &snapshot);
        }
        if let Some(frame) = view_choice.frame {
            self.view_frame = frame;
        }
        // The language is UI state and nobody else's: it is not in the
        // snapshot, does not reach the save, and the world has no business
        // knowing about it (rule 1 of the stage).
        if let Some(language) = view_choice.language {
            self.language = language;
        }

        // The nodes for the next frame: a mouse event arrives between frames
        // and must be compared against what the player saw (U4b).
        self.nodes = match snapshot.vessels.first() {
            Some(vessel) => node::nodes_on_screen(
                &camera,
                engine::frame::FOV_Y,
                self.target.width(),
                self.target.height(),
                vessel,
                &self.draft.manoeuvres,
            ),
            None => Vec::new(),
        };

        // Dragging a handle is the same edit as a field in the panel, so the
        // answer is the same: a preview request (U4a). One per frame rather
        // than per mouse event: cancellation between legs was designed for
        // exactly this (J5).
        if self.draft_changed {
            self.draft_changed = false;
            self.apply_plan_action(hud::PlanAction::Preview(self.draft.plan()), &snapshot);
        }

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(surface);
        Ok(())
    }
}
