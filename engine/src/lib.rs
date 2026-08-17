//! The engine: rendering, window, screenshots (ROADMAP, stage F).
//!
//! The steps here are deliberately small -- F1 window and clear, F2 triangle,
//! then reversed-Z, camera-relative, scale. Each step either works or
//! localises the problem; assembling everything at once means looking for the
//! cause everywhere.
//!
//! ## Two paths to one frame
//!
//! What is drawn lives in [`frame`] and **knows nothing about the window**.
//! The same frame then takes two paths:
//!
//!   [`app`]  to a window through a surface -- what the user sees;
//!   [`shot`] to a texture and a PNG -- what can be looked at without a
//!            window, run in CI and committed beside the code.
//!
//! This is not duplicated work but the only reason rendering can be checked
//! at all. "The window opened and did not crash" is no check: a black frame
//! looks the same.
//!
//! ## The engine knows nothing about the game
//!
//! Since J1 the same decision applies a level up: [`frame`] draws a
//! [`scene::Scene`] -- a camera and geometry -- and knows nothing of vessels,
//! plans or time. The game translates its own snapshot into a scene
//! (PROJECT.md §6). So `engine` never depends on `game`, and [`app`] remains
//! an event loop for the engine's probes, while the game has its own and owns
//! the world.

// The interface comes from the engine rather than from a dependency of the
// game's own (ROADMAP-UI.md, U1b). The re-export is not a convenience: two
// versions of egui in one build cannot arise **by construction** rather than
// by agreement, which is why `game` has no business writing `egui` into its
// Cargo.toml.
pub use egui;
pub use egui_wgpu;

pub mod app;
pub mod atmosphere;
pub mod brdf;
pub mod camera;
pub mod camera_probe;
pub mod chase;
pub mod cubesphere;
pub mod cull;
pub mod demo;
pub mod depth;
pub mod depth_probe;
pub mod detail;
pub mod flight_probe;
pub mod flyby_demo;
pub mod frame;
pub mod gpu;
pub mod live;
pub mod lod;
pub mod material;
pub mod mesh;
pub mod moon_demo;
pub mod orbit;
pub mod perf_probe;
pub mod planetshine;
pub mod rotating_probe;
pub mod scene;
pub mod ship;
pub mod ship_demo;
pub mod shot;
pub mod sky;
pub mod sphere;
pub mod sphere_render;
pub mod srgb;
pub mod stars;
pub mod tile_probe;
pub mod tiles;
pub mod tonemap;
pub mod trajectory;
pub mod trajectory_render;
pub mod ui;
pub mod window;
