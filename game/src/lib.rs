//! The game: world, time, plans, saves (ROADMAP, stage J).
//!
//! ## What is decided here
//!
//! Not the physics and not the picture -- those already exist. What is decided
//! here is **who owns what** (PROJECT.md §6), and one decision drags the rest
//! along:
//!
//! > The clock does not enter the integrator.
//!
//! The future is computed before time reaches it; time only crawls a cursor
//! along an already computed polyline. So no fixed simulation step is needed,
//! and the frame rate cannot change one bit of a trajectory. The condition
//! under which that is true is CLAUDE.md invariant 9: `t_end` in `prop_run`
//! does not come from a clock, and work is measured in **legs** ([`leg`]).
//!
//! ## Direction of dependencies
//!
//! `game -> engine` and `game -> core-rs`, never the other way. The engine
//! receives an `engine::scene::Scene` -- camera and geometry -- and knows
//! nothing of vessels, plans or time ([`view`]). The same decision as "the
//! frame does not know about the window", one level up.
//!
//! ## State as of J4
//!
//! The world lives in its own thread ([`sim`]): commands by channel, snapshots
//! by publication, no shared mutable state. The world's code did not change by
//! a single line in the move -- [`world::World::step`] was an ordinary
//! function and stayed one -- which is exactly why J1-J3 were done
//! single-threaded.
//!
//! Time ([`clock`]): cursor, warp, pause, horizon in legs ahead of the cursor.
//! The manoeuvre plan ([`plan`]) becomes a sequence of `prop_run` calls in
//! segments between manoeuvres; editing the plan cuts the trajectory's tail
//! rather than recomputing it from the epoch.

pub mod app;
pub mod clock;
pub mod frame_view;
pub mod hud;
pub mod leg;
pub mod mission;
pub mod node;
pub mod palette;
pub mod perf_probe;
pub mod plan;
pub mod planner;
pub mod porkchop;
pub mod save;
pub mod schedule;
pub mod sim;
pub mod snapshot;
pub mod text;
pub mod thin;
pub mod trail;
pub mod view;
pub mod world;
pub mod zvc;
