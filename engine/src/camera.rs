//! The camera: position in `double`, camera-relative transform (ROADMAP F4,
//! F5).
//!
//! PROJECT.md §7, decision 1: world coordinates NEVER reach a `float`.
//! [`camera_probe`](crate::camera_probe) proved the principle on a single
//! point; here the same principle is applied to every vertex of a mesh,
//! because on a planet-sized object the camera can be ten metres from one
//! vertex and 1e7 m from the opposite one at the same time.
//!
//! Rotation and translation are computed together, in `double`, before
//! narrowing: the camera basis (right, up, forward) rotates the difference
//! `world - camera`, and only the result -- already a small number -- becomes
//! `f32`. The projection matrix ([`crate::depth`]) then expects ready camera
//! space coordinates, as in `depth_quad.slang`.

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = dot(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

pub struct Camera {
    position: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    forward: [f64; 3],
}

impl Camera {
    /// `world_up` is a hint, not necessarily orthogonal to the view
    /// direction; it is orthogonalised right here (the standard Gram-Schmidt
    /// for three vectors).
    pub fn look_at(position: [f64; 3], target: [f64; 3], world_up: [f64; 3]) -> Camera {
        let forward = normalize(sub(target, position));
        let right = normalize(cross(forward, world_up));
        let up = cross(right, forward);

        Camera {
            position,
            right,
            up,
            forward,
        }
    }

    /// Where the camera stands, in world coordinates.
    ///
    /// Needed by whoever computes something from the distance to the scene --
    /// the near plane for the current altitude, for instance
    /// (`frame::Frame`).
    pub fn position(&self) -> [f64; 3] {
        self.position
    }

    /// The camera's three axes in world coordinates: right, up, forward.
    ///
    /// Needed by the sky pass (S4b): the pixel ray is built from them and from
    /// the tangents of the view half-angles, with no inverse projection
    /// matrix. Inverting a matrix for a direction would introduce a second
    /// truth about the same basis -- exactly what [`Self::view_rotation`]
    /// warns against.
    pub fn axes(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        (self.right, self.up, self.forward)
    }

    /// A world point -> a coordinate in camera space (the camera at (0,0,0)
    /// looking along -z). Subtraction and rotation happen in `double`;
    /// narrowing to `f32` is the last step, when the number is already small.
    pub fn relative(&self, world: [f64; 3]) -> [f32; 3] {
        let v = self.relative64(world);
        [v[0] as f32, v[1] as f32, v[2] as f32]
    }

    /// The same without narrowing to `f32`.
    ///
    /// Needed by frustum culling (R3b): there a patch coordinate is compared
    /// with the radius of its bounding sphere, and both numbers must come out
    /// of the same arithmetic. Narrowing in the middle would make the culling
    /// boundary a property of `f32` rather than of geometry.
    pub fn relative64(&self, world: [f64; 3]) -> [f64; 3] {
        let d = sub(world, self.position);
        [dot(d, self.right), dot(d, self.up), -dot(d, self.forward)]
    }

    /// A world **direction** -> a direction in camera space: rotation without
    /// translation (ROADMAP-PLANETS.md, R1b).
    ///
    /// Needed by the patch. A patch subtracts the camera **once** -- from its
    /// own origin, in `f64` -- while its vertices are offsets in world axes,
    /// so they have to be rotated separately. Doing it through
    /// [`Self::relative`] is impossible by construction: that subtracts the
    /// camera position, which an offset no longer contains.
    ///
    /// Narrowing to `f32` here is safe without caveats: an offset inside a
    /// patch is small by definition and has no catastrophic cancellation in it
    /// -- which is why it stayed the one thing stored as `f32`.
    pub fn rotate(&self, direction: [f64; 3]) -> [f32; 3] {
        [
            dot(direction, self.right) as f32,
            dot(direction, self.up) as f32,
            -dot(direction, self.forward) as f32,
        ]
    }

    /// The view rotation as a 4x4 matrix in the [`crate::depth::Matrix`]
    /// layout (ROADMAP-PLANETS.md, R1d).
    ///
    /// Rotation only, no translation, and that is no oversight: the
    /// translation was already done by subtracting the camera in `f64` -- on
    /// the CPU, once per patch. A matrix with the camera position inside it
    /// would mean subtracting in `f32`, i.e. exactly the catastrophic
    /// cancellation the whole camera-relative scheme saves us from.
    ///
    /// The rows are [`Self::relative`] written as a matrix: `right`, `up`,
    /// `-forward`. The two implementations of the same rotation sit side by
    /// side deliberately, and a test compares them -- because a divergence
    /// between the CPU path and the GPU path is what would give a planet
    /// shifted relative to the polylines.
    pub fn view_rotation(&self) -> [[f32; 4]; 4] {
        let rows = [
            self.right,
            self.up,
            [-self.forward[0], -self.forward[1], -self.forward[2]],
        ];

        let mut m = [[0.0f32; 4]; 4];
        for (col, column) in m.iter_mut().enumerate().take(3) {
            for (row, value) in column.iter_mut().enumerate().take(3) {
                *value = rows[row][col] as f32;
            }
        }
        m[3][3] = 1.0;
        m
    }

    /// A world point -> a pixel on screen, or `None` if it is behind the
    /// camera (ROADMAP-UI.md, U4b).
    ///
    /// Needed for picking: finding the manoeuvre node nearest the cursor is a
    /// comparison in pixels, not a raycast into the scene. A raycast without
    /// an id buffer would be a subsystem of its own for the sake of one
    /// marker.
    ///
    /// **The near plane does not enter this, and that is not a
    /// simplification.** In [`crate::depth::reversed_infinite`] `near` appears
    /// only in the `z` row; `x` and `y` do not depend on it at all, so picking
    /// has no need to know about depth.
    ///
    /// The axes are egui's: `x` to the right, `y` **down**. That way the
    /// number from this function is compared with the cursor position with no
    /// flip along the way.
    pub fn to_screen(
        &self,
        fov_y: f64,
        width: u32,
        height: u32,
        world: [f64; 3],
    ) -> Option<[f32; 2]> {
        let view = self.relative(world);

        // The camera looks along -z, so what is ahead has negative z. Zero is
        // excluded too: the division there is meaningless rather than "a very
        // large number".
        if view[2] >= 0.0 {
            return None;
        }

        let f = 1.0 / (fov_y / 2.0).tan();
        let aspect = f64::from(width) / f64::from(height);
        let depth = f64::from(-view[2]);

        let ndc_x = (f / aspect) * f64::from(view[0]) / depth;
        let ndc_y = f * f64::from(view[1]) / depth;

        Some([
            ((ndc_x + 1.0) * 0.5 * f64::from(width)) as f32,
            ((1.0 - ndc_y) * 0.5 * f64::from(height)) as f32,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_straight_ahead_lands_on_negative_z() {
        let camera = Camera::look_at([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let p = camera.relative([10.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-6);
        assert!((p[1]).abs() < 1e-6);
        assert!((p[2] - (-10.0)).abs() < 1e-6);
    }

    #[test]
    fn distance_to_the_world_origin_does_not_enter_the_result() {
        let near = Camera::look_at([1e3, 0.0, 0.0], [1e3 + 1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let far = Camera::look_at([1e11, 0.0, 0.0], [1e11 + 1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

        let a = near.relative([1e3 + 5.0, 2.0, -3.0]);
        let b = far.relative([1e11 + 5.0, 2.0, -3.0]);

        for i in 0..3 {
            assert!(
                (a[i] - b[i]).abs() < 1e-4,
                "component {i}: {} against {}",
                a[i],
                b[i]
            );
        }
    }
}
