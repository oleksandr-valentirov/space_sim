//! Saving: state, plan and integrator step (ROADMAP J6, PROJECT.md §4).
//!
//! Rule 4 of §4 reads: **the integrator's state goes into the save.** An
//! adaptive step means the step sequence depends on history; starting from a
//! "fresh" step after loading makes the trajectory diverge from the one before
//! saving, and in N-body the divergence grows exponentially.
//!
//! H1 measured the cost on one run: 7148 samples instead of 101, and a 1.9 mm
//! divergence. J3 measured it on a manoeuvre: 1.4% of wasted work. Here the
//! price is different and worst of all -- a save that gives **a different
//! game**.
//!
//! ## What the save does not hold
//!
//! The trajectory. §4: "a save = state + manoeuvre plan + integrator state
//! (not the whole trajectory)". A consequence worth knowing in advance: after
//! loading there is no drawn history -- it is restored only forwards, from the
//! save point. That is a design decision rather than a space saving; if
//! history is ever needed it goes in a separate (and disposable) file.
//!
//! ## Why hexadecimal bits rather than numbers
//!
//! A save must reproduce the game **bitwise**, and decimal printing is an
//! agreement between formatter and parser. In Rust it is reliable (the
//! shortest representation that reads back exactly), but C6 already recorded
//! the opposite case: printing a `double` as decimal text is libc's business,
//! which is exactly why CSV are not part of the determinism comparison. Here
//! the cost of an error exceeds readability, so `to_bits` goes into the file
//! and the decimal value stays beside it as a **comment** -- for the eye, and
//! the parser does not read it.

use std::fmt::Write as _;
use std::path::Path;

use core_rs::{State, Vec3d};

use crate::plan::{Frame, Manoeuvre, Plan};
use crate::world::World;

/// Format version. When it changes, old saves must loudly fail to read rather
/// than quietly read wrongly.
const MAGIC: &str = "space_sim save v1";

pub struct SavedVessel {
    pub name: String,
    /// The state to continue from: the last **leg boundary not later than the
    /// cursor**, not the end of what is computed.
    ///
    /// Not the end, because the prediction ahead of the cursor is not part of
    /// the save, and restoring the game from it would jump weeks forward. Not
    /// the cursor itself, because continuing bitwise from an arbitrary point
    /// is impossible: the integrator step is preserved at leg boundaries and
    /// only there (`core/prop.h`).
    ///
    /// So the save's granularity is a leg. How much time that is depends on
    /// the trajectory, and shrinks along with `world::LEG`.
    pub tip: State,
    /// The integrator step. Without it the save gives a different
    /// trajectory.
    pub step: f64,
    pub horizon_end: f64,
    pub plan: Plan,
    /// How many of the plan's manoeuvres are already baked into `tip`.
    ///
    /// Stored explicitly although it follows from the times: the states before
    /// and after an impulse share **the same time**, so a rule of "apply
    /// everything not later" would execute the manoeuvre twice, and
    /// "everything earlier" not at all, if the restart point ever became
    /// post-impulse. The number in the file settles that question forever.
    pub applied: usize,

    /// Area, mass and reflectivity coefficient (ROADMAP K6b).
    ///
    /// In the save not for descriptive completeness but because without them a
    /// loaded vessel would fly through a different force model than the saved
    /// one, and the trajectory after loading would diverge from the one before
    /// -- exactly what PROJECT.md §4 requires be prevented for the integrator
    /// step, for the same reason and at the same scale.
    pub params: Option<core_rs::VesselParams>,
}

pub struct Save {
    pub t: f64,
    pub warp: f64,
    pub vessels: Vec<SavedVessel>,
}

impl Save {
    /// Takes a save from the world.
    ///
    /// The cursor is stored as is, and each vessel's state comes from the last
    /// leg boundary not later than it. After loading the horizon catches the
    /// cursor up on its own (the clock does not go backwards,
    /// `crate::clock`), computing exactly the same legs that were there.
    pub fn of(world: &World) -> Save {
        let cursor = world.clock().t();

        Save {
            t: cursor,
            warp: world.clock().warp(),
            vessels: world
                .vessels()
                .iter()
                .map(|v| {
                    let resume =
                        crate::leg::restart_at(v.trajectory.legs(), v.trajectory.start(), cursor);

                    SavedVessel {
                        name: v.name.clone(),
                        tip: resume.state,
                        step: resume.step,
                        horizon_end: v.horizon_end,
                        plan: v.plan.clone(),
                        // The restart point is always the state BEFORE the
                        // impulse (the leg ends before it), so a manoeuvre at
                        // exactly this instant is not yet applied.
                        applied: v
                            .plan
                            .manoeuvres()
                            .iter()
                            .take_while(|m| m.t < resume.state.t)
                            .count(),
                        params: v.params,
                    }
                })
                .collect(),
        }
    }

    /// Builds a world from a save on an already loaded ephemeris.
    pub fn into_world(
        self,
        eph: std::sync::Arc<core_rs::Ephemeris>,
        cfg: core_rs::PropConfig,
    ) -> Result<World, core_rs::CoreError> {
        // The clock is set to the saved cursor although there is no
        // trajectory there yet: the horizon catches it up in the first few
        // ticks, and the cursor does not go backwards (`crate::clock`). Until
        // then the snapshot honestly says `Stall::Horizon`.
        let mut world = World::with_ephemeris(eph, cfg, self.t, self.warp)?;

        for vessel in self.vessels {
            world.add_saved_vessel(vessel);
        }

        Ok(world)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_text()).map_err(|e| e.to_string())
    }

    pub fn read(path: &Path) -> Result<Save, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Save::from_text(&text)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{MAGIC}");
        let _ = writeln!(out, "t {} # {:e}", hex(self.t), self.t);
        let _ = writeln!(out, "warp {} # {:e}", hex(self.warp), self.warp);

        for vessel in &self.vessels {
            let _ = writeln!(out, "vessel {}", vessel.name);
            let _ = writeln!(out, "  tip {}", state_line(&vessel.tip));
            let _ = writeln!(out, "  step {} # {:e}", hex(vessel.step), vessel.step);
            let _ = writeln!(
                out,
                "  horizon_end {} # {:e}",
                hex(vessel.horizon_end),
                vessel.horizon_end
            );
            let _ = writeln!(out, "  applied {}", vessel.applied);
            if let Some(p) = vessel.params {
                let _ = writeln!(
                    out,
                    "  params {} {} {} {} # {:e} kg, {:e} m^2, cr {:e}, cd {:e}",
                    hex(p.mass_kg),
                    hex(p.area_m2),
                    hex(p.cr),
                    hex(p.cd),
                    p.mass_kg,
                    p.area_m2,
                    p.cr,
                    p.cd
                );
            }
            for m in vessel.plan.manoeuvres() {
                let frame = match m.frame {
                    Frame::Inertial => "inertial".to_string(),
                    Frame::Vnb { body } => format!("vnb:{body}"),
                };
                let _ = writeln!(
                    out,
                    "  manoeuvre {} {} {} {} {frame} # t={:e} dv=({:e}, {:e}, {:e})",
                    hex(m.t),
                    hex(m.dv[0]),
                    hex(m.dv[1]),
                    hex(m.dv[2]),
                    m.t,
                    m.dv[0],
                    m.dv[1],
                    m.dv[2]
                );
            }
        }

        out
    }

    pub fn from_text(text: &str) -> Result<Save, String> {
        let mut lines = text.lines();

        match lines.next().map(str::trim) {
            Some(MAGIC) => {}
            other => return Err(format!("not a save of this format: {other:?}")),
        }

        let mut t = None;
        let mut warp = None;
        let mut vessels: Vec<SavedVessel> = Vec::new();

        for line in lines {
            // A comment is everything after `#`. That is where the decimal
            // values for the eye live, and the parser knows nothing of them.
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let mut words = line.split_whitespace();
            let key = words.next().unwrap_or("");

            match key {
                "t" => t = Some(number(&mut words, "t")?),
                "warp" => warp = Some(number(&mut words, "warp")?),
                "vessel" => vessels.push(SavedVessel {
                    name: words.collect::<Vec<_>>().join(" "),
                    tip: State::default(),
                    step: 0.0,
                    horizon_end: 0.0,
                    plan: Plan::new(),
                    applied: 0,
                    params: None,
                }),
                _ => {
                    let vessel = vessels
                        .last_mut()
                        .ok_or_else(|| format!("'{key}' before the first 'vessel'"))?;

                    match key {
                        "tip" => {
                            let mut values = [0.0; 7];
                            for (index, slot) in values.iter_mut().enumerate() {
                                *slot = number(&mut words, &format!("tip[{index}]"))?;
                            }
                            vessel.tip = State {
                                r: Vec3d {
                                    x: values[0],
                                    y: values[1],
                                    z: values[2],
                                },
                                v: Vec3d {
                                    x: values[3],
                                    y: values[4],
                                    z: values[5],
                                },
                                t: values[6],
                            };
                        }
                        "step" => vessel.step = number(&mut words, "step")?,
                        "horizon_end" => vessel.horizon_end = number(&mut words, "horizon_end")?,
                        // A missing line is `None`, a massless test particle:
                        // saves written before K6b still read and mean exactly
                        // what they meant.
                        "params" => {
                            let mass_kg = number(&mut words, "params[mass]")?;
                            let area_m2 = number(&mut words, "params[area]")?;
                            let cr = number(&mut words, "params[cr]")?;
                            // Missing means zero, i.e. "this vessel does not
                            // feel air": saves written before K7b still read
                            // and mean exactly what they meant. The same
                            // contract as for the whole `params` line above.
                            let cd = match words.clone().next() {
                                Some(w) if !w.starts_with('#') => number(&mut words, "params[cd]")?,
                                _ => 0.0,
                            };
                            vessel.params = Some(core_rs::VesselParams {
                                mass_kg,
                                area_m2,
                                cr,
                                cd,
                            });
                        }
                        "applied" => {
                            vessel.applied = words
                                .next()
                                .ok_or("applied without a value")?
                                .parse()
                                .map_err(|_| "applied is not a number".to_string())?;
                        }
                        "manoeuvre" => {
                            let t = number(&mut words, "manoeuvre.t")?;
                            let dv = [
                                number(&mut words, "manoeuvre.dv0")?,
                                number(&mut words, "manoeuvre.dv1")?,
                                number(&mut words, "manoeuvre.dv2")?,
                            ];
                            let frame = words.next().ok_or("manoeuvre without a frame")?;
                            let frame = match frame {
                                "inertial" => Frame::Inertial,
                                other => match other.strip_prefix("vnb:") {
                                    Some(body) => Frame::Vnb {
                                        body: body
                                            .parse()
                                            .map_err(|_| format!("frame '{other}'"))?,
                                    },
                                    None => return Err(format!("unknown frame '{other}'")),
                                },
                            };
                            vessel.plan.insert(Manoeuvre { t, dv, frame });
                        }
                        other => return Err(format!("unknown key '{other}'")),
                    }
                }
            }
        }

        Ok(Save {
            t: t.ok_or("the save has no 't'")?,
            warp: warp.ok_or("the save has no 'warp'")?,
            vessels,
        })
    }
}

/// A number as bits. Whoever writes the line appends the comment for the eye
/// -- **one** per line and at the end: `#` eats everything after it.
fn hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn state_line(state: &State) -> String {
    format!(
        "{:016x} {:016x} {:016x} {:016x} {:016x} {:016x} {:016x} \
         # r=({:e}, {:e}, {:e}) t={:e}",
        state.r.x.to_bits(),
        state.r.y.to_bits(),
        state.r.z.to_bits(),
        state.v.x.to_bits(),
        state.v.y.to_bits(),
        state.v.z.to_bits(),
        state.t.to_bits(),
        state.r.x,
        state.r.y,
        state.r.z,
        state.t
    )
}

fn number<'a>(words: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<f64, String> {
    let word = words
        .next()
        .ok_or_else(|| format!("{what} without a value"))?;
    let raw = u64::from_str_radix(word, 16).map_err(|_| format!("{what}: '{word}' is not bits"))?;
    Ok(f64::from_bits(raw))
}

/// Where the game writes by default.
pub fn default_path() -> std::path::PathBuf {
    std::path::PathBuf::from("build/save.txt")
}

/// A convenience for whoever saves the world in the simulation thread.
pub fn write_world(world: &World, path: &Path) -> Result<(), String> {
    Save::of(world).write(path)
}
