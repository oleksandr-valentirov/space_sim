//! Per-leg cache of the thinned trail (ROADMAP.md, N2b).
//!
//! N2a showed that the pixel criterion costs 415 ms per frame -- forty times
//! more than the 9 ms it saves. So thinning is worth it only when computed
//! **not every frame**, and that is what this module does.
//!
//! ## What the cache actually rests on
//!
//! **A sample's point in the frame does not depend on the frame's time --
//! neither in the inertial frame nor in the rotating one.** For the inertial
//! that is obvious: `vessel - Earth` at the sample's instant. For the rotating
//! one it is not, and is worth saying outright: a sample's `Synodic` is built
//! from **its own** Earth-Moon line and its own normal, taking from the frame
//! only `scale` (the constant `SYNODIC_SCALE_M`) and `mass_ratio` (from the
//! asset). So a sample's position in synodic coordinates is fixed from the
//! moment the sample was computed, and can be cached forever rather than until
//! the next frame. `game/tests/trail.rs` checks that, not this comment.
//!
//! ## Why the tolerance is in metres though the criterion is on screen
//!
//! N2a computed the deviation in **pixels**, so the result depended on where
//! the camera looked from -- and rotating the camera would force the cache to
//! be thrown away. Here it is different: the deviation is measured in metres
//! and the tolerance derived from distance -- `tol_px * d / focal_px`, where
//! `d` is the leg's **nearest** point to the camera. A spatial deviation is
//! never smaller than a screen one, so such a tolerance is conservative: it
//! may keep a superfluous vertex but cannot remove a visible one. In exchange
//! it does not depend on view direction, so the cache survives rotating the
//! camera -- which the player does constantly.
//!
//! ## What invalidates an entry
//!
//! A leg is immutable from the moment it was computed, so only a change of
//! scale invalidates -- and not any change, but crossing a power of two. The
//! key is the address of the `Arc<Leg>`, and the entry **holds that same
//! `Arc`**: otherwise a freed leg would give its address to a new one and the
//! cache would quietly answer with someone else's points.

use std::collections::HashMap;
use std::sync::Arc;

use engine::camera::Camera;

use crate::frame_view::{Synodic, ViewFrame};
use crate::leg::Leg;

/// A trail point: the sample's time and its position in the frame being drawn.
///
/// The time is needed because the cursor splits the trail into history and
/// prediction, and after thinning a sample can no longer be found by index.
pub type Point = (f64, [f64; 3]);

struct Entry {
    /// The leg itself -- so its address, which is the key, stays occupied.
    leg: Arc<Leg>,
    frame: ViewFrame,
    /// The power-of-two exponent the tolerance in metres falls in.
    bucket: i32,
    /// The leg's centre and radius in this frame -- fixed, like the points.
    centre: [f64; 3],
    radius: f64,
    points: Vec<Point>,
    /// The frame number this entry was last needed on.
    used: u64,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<usize, Entry>,
    frame: u64,
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    /// How many legs lie in the cache. For tests and the probe.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A new frame: from here `points` marks what this frame needs.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Discard what this frame did not ask for.
    ///
    /// Without this the cache would hold legs the world no longer has -- and
    /// after a plan edit the cascade discards them by the dozen (J3).
    pub fn sweep(&mut self) {
        let frame = self.frame;
        self.entries.retain(|_, entry| entry.used == frame);
    }

    /// The leg's thinned points in a given frame.
    ///
    /// `synodic` is the "now" basis; only the scale and `mu` are taken from
    /// it, both constant (see the module intro). `None` means the inertial
    /// frame.
    pub fn points(
        &mut self,
        leg: &Arc<Leg>,
        frame: ViewFrame,
        synodic: Option<&Synodic>,
        camera: &Camera,
        focal_px: f64,
        tol_px: f64,
    ) -> &[Point] {
        let key = Arc::as_ptr(leg) as usize;
        let now = self.frame;

        // The bounds are needed to learn the scale, and the scale to learn
        // whether the entry fits. So the first pass computes the bounds if
        // there is no entry yet or it belongs to another frame.
        let fresh = match self.entries.get(&key) {
            Some(entry) => Arc::ptr_eq(&entry.leg, leg) && entry.frame == frame,
            None => false,
        };

        if !fresh {
            let points = transform(leg, synodic);
            let (centre, radius) = bounds(&points);
            let bucket = bucket_of(tolerance_m(camera, centre, radius, focal_px, tol_px));
            self.entries.insert(
                key,
                Entry {
                    leg: leg.clone(),
                    frame,
                    bucket,
                    centre,
                    radius,
                    points: thin(&points, exponent_to_metres(bucket)),
                    used: now,
                },
            );
            return &self.entries[&key].points;
        }

        let entry = self.entries.get_mut(&key).expect("just checked");
        entry.used = now;

        let bucket = bucket_of(tolerance_m(
            camera,
            entry.centre,
            entry.radius,
            focal_px,
            tol_px,
        ));
        if bucket != entry.bucket {
            let points = transform(&entry.leg, synodic);
            entry.points = thin(&points, exponent_to_metres(bucket));
            entry.bucket = bucket;
        }

        &entry.points
    }
}

/// The leg's points in the frame they are drawn in.
fn transform(leg: &Leg, synodic: Option<&Synodic>) -> Vec<Point> {
    let normals = crate::view::plane_normals(&leg.samples);
    let mut out = Vec::with_capacity(leg.samples.len());
    for (index, sample) in leg.samples.iter().enumerate() {
        let point = match synodic {
            None => crate::view::geocentric(sample),
            Some(now) => match crate::view::sample_frame(sample, normals[index], now) {
                Some(turned) => turned,
                None => continue,
            },
        };
        out.push((sample.state.t, point));
    }
    out
}

fn bounds(points: &[Point]) -> ([f64; 3], f64) {
    if points.is_empty() {
        return ([0.0; 3], 0.0);
    }

    let mut lo = points[0].1;
    let mut hi = points[0].1;
    for (_, p) in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }

    let centre = [
        (lo[0] + hi[0]) * 0.5,
        (lo[1] + hi[1]) * 0.5,
        (lo[2] + hi[2]) * 0.5,
    ];
    let half = [
        (hi[0] - lo[0]) * 0.5,
        (hi[1] - lo[1]) * 0.5,
        (hi[2] - lo[2]) * 0.5,
    ];
    let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
    (centre, radius)
}

/// The tolerance in metres for a leg whose nearest point is `d` from the
/// camera.
///
/// `d` comes from the bounding sphere and is never smaller than a small
/// fraction of the radius: the camera **inside** the leg's sphere is not "zero
/// metres per pixel" but the same case that cost the frame twice in the
/// patches (D13, D14).
fn tolerance_m(camera: &Camera, centre: [f64; 3], radius: f64, focal_px: f64, tol_px: f64) -> f64 {
    let eye = camera.position();
    let d = [centre[0] - eye[0], centre[1] - eye[1], centre[2] - eye[2]];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - radius;
    // Inside the bounds the nearest point may be right at the eye; there
    // thinning is not allowed at all, and a hundredth of the radius is "almost
    // nothing" rather than a zero, which would give a zero tolerance and no
    // saving.
    let distance = distance.max(radius * 0.01).max(1.0);
    tol_px * distance / focal_px
}

/// The power of two the tolerance falls in.
fn bucket_of(tolerance_m: f64) -> i32 {
    tolerance_m.log2().floor() as i32
}

/// The bucket's lower bound -- that is what is taken as the tolerance.
///
/// The lower bound rather than the middle: the bucket should be **no stricter**
/// than what the camera asked for, and only where that is safe. Taking the
/// lower bound means we always thinned less than allowed, and no vertex
/// disappears early.
fn exponent_to_metres(bucket: i32) -> f64 {
    (bucket as f64).exp2()
}

/// Douglas-Peucker in metres, in frame space.
fn thin(points: &[Point], tol_m: f64) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let plane: Vec<[f64; 3]> = points.iter().map(|&(_, p)| p).collect();
    crate::thin::simplify3(&plane, tol_m)
        .into_iter()
        .map(|index| points[index])
        .collect()
}
