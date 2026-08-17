//! What to draw. The boundary between the engine and the game (ROADMAP J1,
//! PROJECT.md section 6).
//!
//! This continues the already accepted "the frame does not know about the
//! window" one level up: **the engine does not know about the game**. There are
//! no vessels here, no plans and no time -- only a camera and geometry. Who
//! computed these polylines and what they mean, [`crate::frame`] does not ask
//! and must not.
//!
//! The direction of dependencies thereby becomes one-way and stays that way:
//! `game -> engine`, never the reverse. The game translates its snapshot into a
//! [`Scene`] every frame; the translation is cheap, because the polylines have
//! to be narrowed to `f32` relative to the camera anyway, and that is done once,
//! in the frame.
//!
//! ## The list of bodies appeared, and why exactly now
//!
//! Until recently it was not here, and the reason was written down plainly:
//! there was one sphere in the frame, at the origin, with Earth's radius from
//! the engine, and a list of bodies would have been a struct somebody fills in
//! and the engine ignores -- worse than its own absence. The condition lifted:
//! `eph_body_radius` brought the radius (U2a), `eph_body_orientation` the
//! orientation (R1c), and the list finally has a reader -- the cubesphere, which
//! needs to know **where** a body is and **how it is turned**
//! (ROADMAP-PLANETS.md, R1).
//!
//! **This is data, not "Earth".** [`Body`] knows neither a name nor an index in
//! an ephemeris: a centre, a radius, an orientation, a tile-set identifier. The
//! engine still does not know about the game -- rule 3 of stage R.
//!
//! ## What is deliberately absent
//!
//! **The rotating frame.** PROJECT.md section 7 requires it as the default frame
//! for the map, and `trajectory_render` already knows how to compute it in the
//! vertex shader. But there the transform is tied to the Earth-Moon pair and to
//! its own camera by a single offset; dragging that into the interactive frame
//! before a frame service exists would fix that particular pair of bodies into
//! the engine. For now polylines arrive in the world coordinates they are meant
//! to be seen in.

use crate::camera::Camera;

/// A polyline in world coordinates, metres.
///
/// `Vec<[f64; 3]>` is copied every frame, and that is deliberate: a hundred
/// kilobytes per copy against per-leg vertex buffers, which are worth
/// introducing when the profiler asks for them (PROJECT.md section 6 names the
/// leg as the unit of upload for exactly that day).
pub struct Polyline {
    pub points: Vec<[f64; 3]>,
    pub colour: [f32; 4],
}

/// A body in the frame -- **data**, not "Earth" (ROADMAP-PLANETS.md, R1c).
///
/// There is no name and no ephemeris index here and there will be none: the
/// engine does not know about the game, and a planet to it is a centre, a size,
/// a rotation and a tile set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// The centre in world coordinates, metres. `f64`, because this is exactly
    /// the quantity all of camera-relative exists for.
    pub centre: [f64; 3],
    /// The mean radius, metres. From the asset (`eph_body_radius`), not from
    /// the engine.
    pub radius_m: f64,
    /// Rotation from body space into world space: `[w, x, y, z]`.
    ///
    /// An array rather than a type from `core-rs`: the engine knows nothing about
    /// the core, and the `engine -> core-rs` dependency exists only for the
    /// `live` fixture. The identity quaternion means "does not rotate", and that
    /// is what the asset returns for a body without a model.
    pub orientation: [f64; 4],
    /// Which tile set to draw. There is none yet, and the field remains the
    /// **only** thing distinguishing one body from another for the renderer: the
    /// DEM arrives in R5 and will be addressed by exactly this.
    pub tiles: TileSet,
    /// Surface colour (stage T, step T1).
    ///
    /// **A property of the body rather than an engine constant** -- the same
    /// reason as for the radius, the air and the harmonics: the Moon's colour is
    /// a property of the Moon, and two callers have no right to draw two
    /// different Moons. Before T1 this was `frame::COLOUR`, hard-wired into the
    /// frame back in F5, so every body in the world had one colour by
    /// construction.
    ///
    /// Temporarily flat: colour tiles arrive in T3, and then this field becomes
    /// the colour of a body **without** tiles (like `TileSet::Smooth` for
    /// heights).
    pub colour: [f32; 4],
    /// The air around the body, or `None` -- there is none
    /// (ROADMAP-ATMOSPHERE.md, S1).
    ///
    /// **A property of the body rather than a frame setting** -- the same
    /// decision as with the harmonics and the radius (CLAUDE.md): Earth's air is
    /// a property of Earth, and two callers have no right to draw two different
    /// Earths. `None` is the Moon, and it is obliged to give the same frame as
    /// before stage S.
    pub air: Option<Atmosphere>,
}

/// A ship in the frame -- geometry, not a vessel (stage V, step V2).
///
/// Here too the engine does not know about the game: no mass, no plan, no fuel.
/// A centre, an orientation and an **extent** are what the frame is built from,
/// and exactly what it lacked in order to draw anything closer than a planet.
///
/// ## The extent is a field, not an engine constant
///
/// `extent_m` (the bounding-sphere radius) is read by exactly one consumer --
/// [`crate::frame::Frame::near_for`]. The near plane must be closer than the
/// hull, otherwise the ship disappears entirely: before V2 it was derived from
/// the altitude above the nearest body and at a 400 km orbit became 40 km, that
/// is it clipped everything closer than forty kilometres.
///
/// Why a number rather than "take it from the mesh": the mesh lives in the
/// engine while the boundary runs here, and the frame has no right to ask the
/// geometry about something it needs **before** loading it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ship {
    /// The centre in world coordinates, metres.
    pub centre: [f64; 3],
    /// Rotation from ship space into world space: `[w, x, y, z]`, as in
    /// [`Body`]. The ship's nose points along its own `+Z`.
    pub orientation: [f64; 4],
    /// Hull height, metres -- the same as in [`crate::ship::generate`].
    pub height_m: f64,
    /// Bounding-sphere radius, metres. Not derived from `height_m`, because the
    /// fins stick out past the hull, and a future real model will stick out
    /// however it likes.
    pub extent_m: f64,
    /// The hull's base colour, **linear light**.
    ///
    /// For a dielectric this is the diffuse albedo, for a metal `F0`, that is the
    /// colour of the reflection itself. Which of the two is decided by
    /// [`Ship::metallic`].
    pub colour: [f32; 4],
    /// Roughness, `0..1`; smaller means a narrower and brighter highlight.
    ///
    /// WARNING: an artistic parameter, not `alpha`: the BRDF takes `roughness^2`
    /// ([`crate::brdf`]), and the same convention holds in glTF and Blender. So a
    /// number from there can be put here without conversion, and that is exactly
    /// what it is for.
    pub roughness: f32,
    /// Metal (`1`) or dielectric (`0`); intermediate values are a mixture.
    ///
    /// Physically there are no intermediates: a material either has free
    /// electrons or it does not. The field stays continuous because Blender and
    /// glTF give it that way, and snapping their values to two would silently
    /// change what was imported.
    pub metallic: f32,
}

/// A body's atmosphere by the Hillaire 2020 model (PROJECT.md section 7).
///
/// The units are **per metre**, not per kilometre: the paper works in
/// kilometres, and that conversion is the easiest place to lose three orders of
/// magnitude. The rest of the engine measures in metres, so these numbers do
/// too.
///
/// Earth's values are [`Atmosphere::EARTH`]; for another planet these are simply
/// different numbers, and they change no code at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// The radius of the atmosphere's upper boundary, metres. From the body's
    /// centre, not from the surface: the same as `radius_m`.
    pub top_m: f64,
    /// Rayleigh scattering at surface level, 1/m, per RGB.
    pub rayleigh_scattering: [f32; 3],
    /// Rayleigh scale height, metres.
    pub rayleigh_height_m: f32,
    /// Mie scattering at surface level, 1/m. Grey -- Mie barely depends on
    /// wavelength, and that is why haze is white.
    pub mie_scattering: f32,
    /// Mie absorption, 1/m. Larger than the scattering: aerosol also
    /// extinguishes.
    pub mie_absorption: f32,
    /// Mie scale height, metres.
    pub mie_height_m: f32,
    /// Asymmetry of the Mie phase function, dimensionless. Positive is
    /// forward.
    pub mie_g: f32,
    /// Ozone absorption at the peak, 1/m, per RGB.
    pub ozone_absorption: [f32; 3],
    /// The ozone layer's centre and half-width, metres. The layer is triangular
    /// as in the paper: rising linearly to the centre and falling linearly.
    pub ozone_centre_m: f32,
    pub ozone_width_m: f32,
}

impl Atmosphere {
    /// Earth: the numbers from the Hillaire 2020 paper, converted to metres.
    ///
    /// The upper boundary at 100 km above the surface is the Karman line and the
    /// same altitude at which `core/atmosphere.c` stops computing drag. The
    /// coincidence is neither accidental nor obligatory: one number is about
    /// rendering, the other about physics, and if they ever diverge it will be a
    /// decision rather than a bug.
    pub const EARTH: Atmosphere = Atmosphere {
        top_m: 6_371_000.0 + 100_000.0,
        // 5.802, 13.558, 33.1e-6 per kilometre in the paper.
        rayleigh_scattering: [5.802e-6, 13.558e-6, 33.1e-6],
        rayleigh_height_m: 8_000.0,
        mie_scattering: 3.996e-6,
        mie_absorption: 4.40e-6,
        mie_height_m: 1_200.0,
        mie_g: 0.8,
        ozone_absorption: [0.650e-6, 1.881e-6, 0.085e-6],
        ozone_centre_m: 25_000.0,
        ozone_width_m: 15_000.0,
    };

    /// How far the air rises above Earth's surface, metres. The Karman line.
    pub const EARTH_THICKNESS_M: f64 = 100_000.0;

    /// The same coefficients, but with the upper boundary above **this** radius.
    ///
    /// The body's radius comes from the asset (`eph_body_radius`), and taking a
    /// constant instead would mean two different Earths: one in physics, another
    /// in the air. The layer's thickness stays the same -- it is a property of
    /// the atmosphere, not of the body.
    pub fn with_surface(self, surface_m: f64) -> Atmosphere {
        Atmosphere {
            top_m: surface_m + (self.top_m - 6_371_000.0),
            ..self
        }
    }

    /// The air layer's thickness, metres: how far the atmosphere rises above the
    /// surface of a body of radius `surface_m`.
    pub fn thickness_m(&self, surface_m: f64) -> f64 {
        self.top_m - surface_m
    }
}

/// A surface tile set.
///
/// Not an `Option`, and that is more honest: "there is no surface" is also a
/// choice of what to draw (a smooth sphere) rather than the absence of a
/// choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileSet {
    /// A smooth sphere without terrain.
    Smooth,
    /// Terrain already loaded into the frame (`Frame::load_terrain`, R5c).
    ///
    /// An identifier rather than a file path: the game says **what** to draw, not
    /// where to get it. The engine still does not know about the game, nor the
    /// game about the tileset format.
    Loaded(TerrainId),
}

/// A terrain handle issued by the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainId(pub usize);

/// The frame as the game sees it: where we look from and what is in it.
pub struct Scene {
    pub camera: Camera,
    pub polylines: Vec<Polyline>,
    /// Bodies in the frame. An empty list is an empty sky, not "draw Earth by
    /// default".
    pub bodies: Vec<Body>,
    /// Ships in the frame (stage V). An empty list is the pre-V2 frame, byte for
    /// byte.
    pub ships: Vec<Ship>,
    /// The direction **to** the light, world axes (stage V, step V5; debt D16).
    ///
    /// A field of the scene rather than of a body: there is one light per frame,
    /// and the ship's hull, the sky and the planet's surface must be lit from one
    /// point. A body carrying its own sun would let them diverge.
    ///
    /// WARNING: **the length matters for bodies and for the ship, but not for the
    /// sky.** The sky normalises the vector itself, while the diffuse term of the
    /// surface and the hull multiplies by it as it is -- a legacy of the
    /// temporary lighting hard-wired back in F5. The game supplies a unit vector;
    /// the frame will normalise it itself when the M5 materials replace the
    /// temporary lighting.
    pub sun: [f64; 3],
    /// The multiplier applied before the tonemapper's curve (stage Z, step Z1).
    ///
    /// A field of the scene rather than of the engine: how much light is in
    /// the frame is known by whoever assembled it. The sun's disc is orders of
    /// magnitude brighter than the lit surface and stage Y's night lights are
    /// two or three orders dimmer than it -- the same scene has to be able to
    /// ask for either.
    ///
    /// ⚠ **One is a decision here, not a placeholder.** At `1.0` the multiplier
    /// does nothing, the curve stays the identity below the knee, and every
    /// frame drawn before Z1 comes out bit for bit the same. Every oracle of
    /// stage T rests on that.
    ///
    /// **There is no automatic exposure and none is planned.** A factor that
    /// drifted with the contents of the frame would dim the faint exactly when
    /// something bright entered it: stage Y's night lights would go out at the
    /// terminator, the one place they are supposed to appear.
    pub exposure: f64,
}

impl Scene {
    /// An empty scene: a camera and nothing else.
    ///
    /// Literally empty -- since R1e the frame draws bodies from here, so such a
    /// scene gives exactly an empty sky. The Earth-radius body the frame used to
    /// substitute itself moved into [`crate::frame::default_scene`] -- to the
    /// engine probes, which are the ones that need it.
    pub fn new(camera: Camera) -> Scene {
        Scene {
            camera,
            polylines: Vec::new(),
            bodies: Vec::new(),
            ships: Vec::new(),
            // The same direction that was the constant `frame::LIGHT_DIR` from
            // F5 to V5 -- and precisely for that reason the engine probes' frame
            // stays bitwise the same. Whoever knows where their light is sets
            // it.
            sun: crate::frame::LIGHT_DIR.map(f64::from),
            exposure: crate::tonemap::DEFAULT_EXPOSURE,
        }
    }

    /// How many vertices all the polylines have together.
    ///
    /// Needed by whoever allocates a buffer for them once rather than every
    /// frame.
    pub fn vertex_count(&self) -> usize {
        self.polylines.iter().map(|p| p.points.len()).sum()
    }
}

#[cfg(test)]
mod atmosphere_tests {
    use super::*;

    /// The units are per metre, and that is the easiest mistake to make
    /// silently.
    ///
    /// The optical depth of a vertical ray through the whole atmosphere is
    /// `beta*H` up to `exp(-thickness/H)`, that is a number of order 0.1 in the
    /// blue for Earth. Had the coefficients stayed "per kilometre", it would come
    /// out at 100 -- a sky nothing is visible through.
    #[test]
    fn the_vertical_optical_depth_is_the_order_of_a_tenth() {
        let air = Atmosphere::EARTH;
        let h = f64::from(air.rayleigh_height_m);
        for (channel, beta) in air.rayleigh_scattering.iter().enumerate() {
            let depth = f64::from(*beta) * h;
            assert!(
                (0.01..1.0).contains(&depth),
                "channel {channel}: optical depth {depth}, so the units are wrong"
            );
        }
    }

    /// Mie extinguishes more than it scatters, and ozone absorbs most in the
    /// green.
    ///
    /// Not aesthetics but a check that the numbers were not swapped: exactly that
    /// kind of mistake is invisible in the frame -- the sky stays blue.
    #[test]
    fn the_coefficients_keep_the_order_the_paper_gives_them() {
        let air = Atmosphere::EARTH;
        assert!(air.mie_absorption > air.mie_scattering);
        assert!(air.mie_height_m < air.rayleigh_height_m);
        assert!(air.rayleigh_scattering[2] > air.rayleigh_scattering[0]);
        assert!(air.ozone_absorption[1] > air.ozone_absorption[0]);
        assert!(air.ozone_absorption[1] > air.ozone_absorption[2]);
    }

    /// The upper boundary follows the body's radius, the layer's thickness
    /// stays.
    #[test]
    fn the_layer_keeps_its_thickness_on_any_radius() {
        let surface = 6_378_137.0;
        let air = Atmosphere::EARTH.with_surface(surface);
        assert_eq!(air.thickness_m(surface), Atmosphere::EARTH_THICKNESS_M);
        assert_eq!(air.rayleigh_height_m, Atmosphere::EARTH.rayleigh_height_m);
    }
}
