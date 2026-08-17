//! A demonstration of the renderer's current state: a series of shots with no
//! window.
//!
//! Not a new path and not a separate renderer. This is the same
//! [`crate::frame::Frame`], the same [`crate::shot`] and the same scenes the
//! tests use -- just collected into one run and captioned. So the demo
//! **cannot** show anything that is not in the game: if a picture came out, the
//! engine really does draw it.
//!
//! ## Why shots rather than a window
//!
//! For the same reason `--shot` exists (ROADMAP F1): a window proves nothing,
//! while a shot can be committed, sent and compared. The demo is reproduced by
//! one command and gives the same bytes:
//!
//! ```sh
//! make cook-dem                                  # once, if assets/ is empty
//! cargo run --release -p engine -- --demo build/demo
//! ```
//!
//! The directory belongs to whoever named it: the demo writes its files into it
//! and **deletes nothing**. A renamed frame leaves the old file behind, and
//! removing it is the business of whoever renamed it.
//!
//! Captions stay in Ukrainian deliberately: they are written into a manifest
//! for the developer, the same class of text as README.md, not diagnostics.
//!
//! ## What is deliberately absent
//!
//! **No shader, colour or camera of its own "to look nicer".** The lighting is
//! temporary and looks it; the planet's colour is the same `COLOUR` from
//! `frame.rs`. A separately tinted demo would be showing itself rather than the
//! engine.

use std::path::Path;

use crate::camera::Camera;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::{Body, Polyline, Scene, TerrainId, TileSet};
use crate::shot;
use crate::tiles::Terrain;
use crate::{live, sphere};

/// The demo frame size.
///
/// Not 1280x720: the shots are read side by side rather than one at a time, and
/// a file four times smaller matters more here than a pixel four times
/// larger.
const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

const MOON_RADIUS_M: f64 = 1_737_400.0;

/// The cooked lunar terrain, relative to the repository root.
pub const TERRAIN_ASSET: &str = "assets/moon.dem";

/// The cooked lunar colour (stage T, T2d). A separate asset, and it can be
/// missing independently of the terrain -- then the demo draws a grey Moon with
/// mountains.
pub const COLOUR_ASSET: &str = "assets/moon.col";

/// One demo frame: the file name and a caption of what is visible on it.
pub struct Picture {
    pub name: &'static str,
    pub caption: String,
}

/// A camera at altitude `altitude` above direction `direction`, looking at the
/// centre.
fn above(direction: [f64; 3], radius_m: f64, altitude: f64, up: [f64; 3]) -> Camera {
    let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
    let distance = radius_m + altitude;
    let eye = direction.map(|v| v / length * distance);
    Camera::look_at(eye, [0.0, 0.0, 0.0], up)
}

/// The unit direction to the light source -- the same one that lights the
/// frame.
fn light() -> [f64; 3] {
    let l = frame::LIGHT_DIR.map(f64::from);
    let n = l.iter().map(|v| v * v).sum::<f64>().sqrt();
    l.map(|v| v / n)
}

/// A direction `tilt` degrees away from the light.
///
/// Needed exactly so as not to shoot the night side: illumination there is
/// constant, and terrain on it is invisible both with tiles and without. The
/// first version of the R5c tests stepped on precisely that.
fn from_light(tilt: f64) -> [f64; 3] {
    let l = light();
    let seed = if l[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let e1 = unit(cross(l, seed));
    let (c, s) = (tilt.to_radians().cos(), tilt.to_radians().sin());
    [0, 1, 2].map(|k| c * l[k] + s * e1[k])
}

/// A camera at altitude `altitude` looking **along the limb** rather than down.
///
/// A nadir view from low orbit gives a flat field of colour and shows nothing:
/// the sphere covers the frame entirely, and a smooth sphere has nothing to
/// show anyway. The first version of the demo came out exactly like that, and
/// it was honest but useless. The limb instead shows everything at once --
/// curvature, the near plane, horizon culling and the terrain profile against
/// the sky.
///
/// The look-at target is the surface point exactly on the horizon:
/// `acos(R / (R + h))` from the sub-camera point. Arithmetic computes it, not
/// the eye.
fn along_limb(direction: [f64; 3], radius_m: f64, altitude: f64) -> Camera {
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let u = unit(direction);
    // A tangent to the sphere at the sub-camera point: any one, as long as it
    // is perpendicular.
    let seed = if u[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = unit([
        u[1] * seed[2] - u[2] * seed[1],
        u[2] * seed[0] - u[0] * seed[2],
        u[0] * seed[1] - u[1] * seed[0],
    ]);
    let tangent = [
        side[1] * u[2] - side[2] * u[1],
        side[2] * u[0] - side[0] * u[2],
        side[0] * u[1] - side[1] * u[0],
    ];

    let distance = radius_m + altitude;
    let eye = u.map(|v| v * distance);
    let horizon = (radius_m / distance).acos();
    let (c, s) = (horizon.cos(), horizon.sin());
    let target = [0, 1, 2].map(|k| radius_m * (c * u[k] + s * tangent[k]));
    // The frame's up is outward from the body: sky on top, surface below.
    Camera::look_at(eye, target, u)
}

fn body(radius_m: f64, tiles: TileSet) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        colour: crate::frame::COLOUR,
        air: None,
    }
}

/// The halo-orbit scene: Earth, the Moon and a trajectory computed **now**.
///
/// The only demo scene with physics in it. The line is the output of `prop_run`
/// through the field of the asset's ten bodies (H5), not a column from a CSV.
///
/// ## Why in the rotating frame rather than in world coordinates
///
/// The first version of this scene drew world coordinates and gave a straight
/// line across the whole frame -- and that is not a render bug but the truth
/// about scale: over twelve days the Earth-Moon system travels heliocentrically
/// orders of magnitude farther than the span of the halo orbit itself. The same
/// reason `trajectory_render` takes a **geocentric** anchor (F6).
///
/// Here the frame is rotating (`trajectory::rotating_position`), and the scale
/// is **pinned** by a constant distance rather than the instantaneous one:
/// otherwise the Moon would breathe along with its orbital eccentricity
/// (U6a3).
fn halo() -> Result<Scene, String> {
    // The mean Earth-Moon distance. The rotating frame is dimensionless, and
    // this constant is what brings it back to metres.
    const L: f64 = 3.844e8;

    let asset = live::repo_asset();
    let flight =
        live::propagate(&live::fixture_start(), 14.0, &asset).map_err(|e| format!("{e:?}"))?;
    let samples = &flight.samples;
    if samples.len() < 2 {
        return Err("the prediction returned fewer than two samples".to_string());
    }

    let points: Vec<[f64; 3]> = samples
        .iter()
        .map(|s| {
            let p = crate::trajectory::rotating_position(s.vessel, s.earth, s.moon, s.z_axis);
            [p[0] * L, p[1] * L, p[2] * L]
        })
        .collect();

    let moon = [(1.0 - crate::trajectory::MU) * L, 0.0, 0.0];
    let earth = [-crate::trajectory::MU * L, 0.0, 0.0];

    // The framing is built from the data: the centre is the middle of the
    // point cloud together with the Moon, the distance comes from its span.
    // Tuning this by hand would mean the shot stops being right the moment the
    // orbit changes.
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in points.iter().chain(std::iter::once(&moon)) {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    let centre = [0, 1, 2].map(|k| (low[k] + high[k]) / 2.0);
    let extent = (0..3)
        .map(|k| high[k] - low[k])
        .fold(0.0_f64, f64::max)
        .max(1.0);

    // A view from the side and slightly above: a halo orbit is not planar, and
    // a head-on view would show it as a line segment.
    let eye = [
        centre[0] - extent * 0.35,
        centre[1] - extent * 1.25,
        centre[2] + extent * 0.55,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, centre, [0.0, 0.0, 1.0]));
    scene.bodies.push(Body {
        centre: earth,
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: crate::frame::COLOUR,
        air: None,
    });
    scene.bodies.push(Body {
        centre: moon,
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: crate::frame::COLOUR,
        air: None,
    });
    scene.polylines.push(Polyline {
        points,
        colour: [1.0, 0.75, 0.25, 1.0],
    });
    Ok(scene)
}

/// Draw the whole series into directory `out`.
pub fn render(gpu: &Gpu, out: &Path) -> Result<Vec<Picture>, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);

    // The terrain comes from a cooked asset. Its absence is not silent: the
    // tiled scenes disappear, and that is said out loud, with the command that
    // fixes it.
    let terrain: Option<TerrainId> = match std::fs::read(TERRAIN_ASSET) {
        Ok(bytes) => {
            let data = Terrain::from_bytes(&bytes)?;
            let levels = data.levels;
            let colour = match std::fs::read(COLOUR_ASSET) {
                Ok(bytes) => {
                    let colour = crate::tiles::Colour::from_bytes(&bytes)?;
                    println!("colour: {COLOUR_ASSET}, {} pyramid levels", colour.levels);
                    Some(colour)
                }
                Err(e) => {
                    println!("no colour ({COLOUR_ASSET}: {e}) -- the Moon is grey.");
                    println!("to fix: make cook-colour");
                    None
                }
            };
            let id = frame.load_surface(gpu, &data, colour.as_ref())?;
            println!("terrain: {TERRAIN_ASSET}, {levels} pyramid levels");
            Some(id)
        }
        Err(e) => {
            println!("no terrain ({TERRAIN_ASSET}: {e}) -- the tiled scenes are skipped.");
            println!("to fix: make cook-dem");
            None
        }
    };

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("demo"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;

    let mut taken = Vec::new();
    let mut shoot = |name: &'static str, caption: String, scene: &Scene| -> Result<(), String> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("demo"),
            });
        frame.draw(gpu, &mut encoder, &view, WIDTH, HEIGHT, scene);
        let picture = shot::read_back(gpu, encoder, &texture, WIDTH, HEIGHT)?;
        let path = out.join(format!("{name}.png"));
        picture.write_png(&path)?;
        println!("  {}", path.display());
        taken.push(Picture { name, caption });
        Ok(())
    };

    // 1. Earth from afar -- the same frame `--shot` gives.
    let mut scene = Scene::new(above(
        from_light(35.0),
        sphere::EARTH_RADIUS_M,
        frame::DEFAULT_ALTITUDE_M,
        [0.0, 0.0, 1.0],
    ));
    scene
        .bodies
        .push(body(sphere::EARTH_RADIUS_M, TileSet::Smooth));
    shoot(
        "01_earth_far",
        "Земля з 10⁷ м. LOD віддає планеті шість граней куба — на цій \
         відстані дрібніший поділ не зсунув би жодного пікселя."
            .to_string(),
        &scene,
    )?;

    // 2. Earth from low orbit, looking along the limb.
    let mut scene = Scene::new(along_limb(from_light(35.0), sphere::EARTH_RADIUS_M, 3.0e5));
    scene
        .bodies
        .push(body(sphere::EARTH_RADIUS_M, TileSet::Smooth));
    shoot(
        "02_earth_low",
        "Земля з 300 км, погляд уздовж лімба. Той самий критерій дав дев'ять \
         патчів замість шести, і горизонт прибрав більше половини з них ще до \
         малювання. Ближня площина міряється від найближчого тіла сцени — \
         інакше на цій висоті вона зрізала б поверхню під ногами."
            .to_string(),
        &scene,
    )?;

    if let Some(id) = terrain {
        // 3 and 4 -- a pair from one camera: without tiles and with them.
        for (name, tiles, what) in [
            ("03_moon_smooth", TileSet::Smooth, "без тайлів"),
            ("04_moon_terrain", TileSet::Loaded(id), "з тайлами LOLA"),
        ] {
            let mut scene = Scene::new(above(
                from_light(30.0),
                MOON_RADIUS_M,
                1.2e6,
                [0.0, 0.0, 1.0],
            ));
            scene.bodies.push(body(MOON_RADIUS_M, tiles));
            shoot(
                name,
                format!(
                    "Місяць з 1.2·10⁶ м, {what}. Пара знімків з однієї камери: \
                     різницю дає рівно висота, зсунута вздовж нормалі патча."
                ),
                &scene,
            )?;
        }

        // 5. The terminator -- what R5c was done for.
        let mut scene = Scene::new(above(from_light(72.0), MOON_RADIUS_M, 1.2e6, light()));
        scene.bodies.push(body(MOON_RADIUS_M, TileSet::Loaded(id)));
        shoot(
            "05_moon_terminator",
            "Місяць на термінаторі. Сонце падає навскіс, і нахил кожної \
             фасетки вирішує, освітлена вона чи ні: повна варіація яскравості \
             тут у 9.7 раза більша, ніж у гладкої сфери."
                .to_string(),
            &scene,
        )?;

        // 6 and 7 -- a second pair, up close and along the limb. A pair rather
        // than one shot for the same reason as above: "the terrain is visible"
        // without a second picture beside it is a claim nobody can check.
        for (name, tiles, what) in [
            ("06_moon_limb_smooth", TileSet::Smooth, "без тайлів"),
            ("07_moon_limb_terrain", TileSet::Loaded(id), "з тайлами"),
        ] {
            let mut scene = Scene::new(along_limb(from_light(35.0), MOON_RADIUS_M, 1.0e5));
            scene.bodies.push(body(MOON_RADIUS_M, tiles));
            shoot(
                name,
                format!(
                    "Місяць зі 100 км уздовж лімба, {what}. Горизонт за 570 км, \
                     і рельєф там міняє сам силует. Фасетки видно, і це чесно: \
                     LDEM_4 дає 7581 м на відлік, тобто 47 екранних пікселів на \
                     клітинку — деталь нижче за дані це вже крок R7."
                ),
                &scene,
            )?;
        }
    }

    // 8. Physics in the frame: a halo orbit computed now.
    match halo() {
        Ok(scene) => shoot(
            "08_halo",
            "Halo-орбіта, порахована `prop_run` крізь поле десяти тіл ассета — \
             не прочитана з CSV. Фрейм обертовий, масштаб закріплений сталою \
             відстанню; Місяць праворуч, Земля за кадром ліворуч. Кадр \
             будується з даних, а не підібраний руками."
                .to_string(),
            &scene,
        )?,
        Err(e) => println!("the halo scene was skipped: {e}"),
    }

    Ok(taken)
}
