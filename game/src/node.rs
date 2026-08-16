//! A manoeuvre node on screen: picking and handles (ROADMAP-UI.md, U4b).
//!
//! ## Picking by projection, not by raycast
//!
//! The camera already knows `to_screen`, so "which node is under the cursor"
//! is a comparison in pixels. A raycast into the scene without an identifier
//! buffer would be a whole subsystem for the sake of one marker.
//!
//! ## Axis handles rather than free dragging
//!
//! This is the step's fork that measurement settled in favour of the fallback,
//! and the reason is geometric rather than a matter of taste. Dragging an
//! arbitrary point in 3D with a mouse is ambiguous -- there is nothing to set
//! the depth with. But the main point is different: **the VNB axes projected
//! to the screen are not orthogonal**. If a drag were decomposed by projection
//! onto all three at once, motion along the screen `normal` would change
//! `prograde` too -- exactly what the step's check forbids. With handles the
//! requirement holds **by construction**: grab one axis and one component
//! moves.

use engine::camera::Camera;

use crate::plan::Manoeuvre;
use crate::snapshot::VesselSnapshot;

/// Handle length from the node, pixels. Far enough not to stick together,
/// close enough not to run off the frame in small windows.
pub const HANDLE_PX: f32 = 60.0;

/// How close to a handle a click must land, pixels.
pub const GRAB_PX: f32 = 14.0;

/// How many metres per second one pixel of drag adds.
///
/// The number here is tool sensitivity, not physics: on a typical orbit
/// manoeuvres are units and tens of m/s, and 0.1 m/s per pixel gives the full
/// range over a few hundred pixels of movement.
pub const M_S_PER_PX: f64 = 0.1;

/// A manoeuvre node as it appears on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeOnScreen {
    /// The manoeuvre's index in the draft.
    pub index: usize,
    /// Where the node is, pixels.
    pub at: [f32; 2],
    /// Where the VNB axes point from the node -- **unit** directions in
    /// pixels. An axis pointing straight at the camera degenerates to zero,
    /// and then it simply has no handle.
    pub axes: [[f32; 2]; 3],
}

impl NodeOnScreen {
    /// Where the handle for axis `axis` is drawn.
    pub fn handle(&self, axis: usize) -> [f32; 2] {
        [
            self.at[0] + self.axes[axis][0] * HANDLE_PX,
            self.at[1] + self.axes[axis][1] * HANDLE_PX,
        ]
    }
}

/// A grabbed handle: which node and which of its axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grab {
    pub node: usize,
    pub axis: usize,
}

/// Projects the draft's nodes to the screen.
///
/// The vessel's state at the manoeuvre's instant comes from **already computed
/// samples** (rule 5: the panel does not propagate). A manoeuvre the
/// prediction has not reached has no node -- showing it in an arbitrary place
/// would be worse than not showing it.
pub fn nodes_on_screen(
    camera: &Camera,
    fov_y: f64,
    width: u32,
    height: u32,
    vessel: &VesselSnapshot,
    manoeuvres: &[Manoeuvre],
) -> Vec<NodeOnScreen> {
    let mut nodes = Vec::new();

    for (index, manoeuvre) in manoeuvres.iter().enumerate() {
        let Some(there) = sample_at(vessel, manoeuvre.t) else {
            continue;
        };
        let Some(at) = camera.to_screen(fov_y, width, height, there.vessel_r) else {
            continue;
        };

        // The VNB axes in world space -- the same triple
        // `Manoeuvre::dv_inertial` expands dv with. One basis, not two similar
        // ones.
        let r = [
            there.vessel_r[0] - there.body_r[0],
            there.vessel_r[1] - there.body_r[1],
            there.vessel_r[2] - there.body_r[2],
        ];
        let v = [
            there.vessel_v[0] - there.body_v[0],
            there.vessel_v[1] - there.body_v[1],
            there.vessel_v[2] - there.body_v[2],
        ];
        let prograde = normalize(v);
        let normal = normalize(cross(r, v));
        let outward = cross(prograde, normal);

        // The world-space length an axis is drawn at, chosen so it is visible
        // on screen both in low orbit and near the Moon. One percent of the
        // distance to the body: the scene's scale sets itself.
        let length = 0.01 * norm(r).max(1.0);
        let mut axes = [[0.0f32; 2]; 3];

        for (axis, direction) in [prograde, normal, outward].iter().enumerate() {
            let tip = [
                there.vessel_r[0] + direction[0] * length,
                there.vessel_r[1] + direction[1] * length,
                there.vessel_r[2] + direction[2] * length,
            ];
            let Some(tip_px) = camera.to_screen(fov_y, width, height, tip) else {
                continue;
            };
            let d = [tip_px[0] - at[0], tip_px[1] - at[1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len > 1e-3 {
                axes[axis] = [d[0] / len, d[1] / len];
            }
        }

        nodes.push(NodeOnScreen { index, at, axes });
    }

    nodes
}

/// Which handle was grabbed, if any.
///
/// The nearest within [`GRAB_PX`]; a handle degenerated to zero (its axis
/// pointing at the camera) cannot be grabbed -- otherwise three handles would
/// coincide at one point and the choice between them would be arbitrary.
pub fn pick_handle(nodes: &[NodeOnScreen], cursor: [f32; 2]) -> Option<Grab> {
    let mut best: Option<(f32, Grab)> = None;

    for node in nodes {
        for axis in 0..3 {
            if node.axes[axis] == [0.0, 0.0] {
                continue;
            }
            let at = node.handle(axis);
            let d = [at[0] - cursor[0], at[1] - cursor[1]];
            let distance = (d[0] * d[0] + d[1] * d[1]).sqrt();

            if distance <= GRAB_PX && best.is_none_or(|(was, _)| distance < was) {
                best = Some((
                    distance,
                    Grab {
                        node: node.index,
                        axis,
                    },
                ));
            }
        }
    }

    best.map(|(_, grab)| grab)
}

/// How many m/s a drag of `drag_px` pixels on a grabbed handle adds.
///
/// The **projection onto its axis** is what counts, so movement across the
/// handle does nothing. The sign is direct: drag the way the axis points and
/// the component grows.
pub fn drag_to_delta(node: &NodeOnScreen, axis: usize, drag_px: [f32; 2]) -> f64 {
    let a = node.axes[axis];
    let along = f64::from(a[0] * drag_px[0] + a[1] * drag_px[1]);
    along * M_S_PER_PX
}

/// Vessel and reference body at one instant -- everything the VNB basis is
/// built from.
#[derive(Clone, Copy, Debug)]
struct At {
    vessel_r: [f64; 3],
    vessel_v: [f64; 3],
    body_r: [f64; 3],
    /// The body's velocity is a finite difference over adjacent samples: a
    /// sample carries only position (`crate::leg`).
    body_v: [f64; 3],
}

/// The vessel's state at time `t` -- the nearest sample, together with the
/// body.
fn sample_at(vessel: &VesselSnapshot, t: f64) -> Option<At> {
    let mut best: Option<(f64, At)> = None;

    for leg in &vessel.legs {
        for (i, sample) in leg.samples.iter().enumerate() {
            let gap = (sample.state.t - t).abs();
            if best.is_some_and(|(was, _)| gap >= was) {
                continue;
            }

            let neighbour =
                leg.samples
                    .get(i + 1)
                    .or_else(|| if i > 0 { leg.samples.get(i - 1) } else { None });
            let body_v = match neighbour {
                Some(other) => {
                    let dt = other.state.t - sample.state.t;
                    if dt == 0.0 {
                        [0.0; 3]
                    } else {
                        [
                            (other.earth[0] - sample.earth[0]) / dt,
                            (other.earth[1] - sample.earth[1]) / dt,
                            (other.earth[2] - sample.earth[2]) / dt,
                        ]
                    }
                }
                None => [0.0; 3],
            };

            best = Some((
                gap,
                At {
                    vessel_r: [sample.state.r.x, sample.state.r.y, sample.state.r.z],
                    vessel_v: [sample.state.v.x, sample.state.v.y, sample.state.v.z],
                    body_r: sample.earth,
                    body_v,
                },
            ));
        }
    }

    best.map(|(_, at)| at)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n == 0.0 {
        [0.0; 3]
    } else {
        [a[0] / n, a[1] / n, a[2] / n]
    }
}
