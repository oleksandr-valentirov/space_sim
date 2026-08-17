//! The game loads the star catalogue (stage Z).
//!
//! Stage Z built the format, the cooker, the pass and its oracles -- and left
//! the cooked catalogue with no reader at all: `assets/stars.cat` was named in
//! exactly two places in the tree, both inside the cooker that writes it. The
//! sky in the game's window was therefore black while every test of the pass
//! passed, because the tests build their star lists in memory.
//!
//! So what is checked here is the seam nobody had: **a file on disk becomes a
//! sky in the frame**. Two halves, and the second is not the smaller one --
//! the game must survive a catalogue that is missing or damaged, since the
//! asset is deliberately not in git.
//!
//! Why the loader takes a path: an oracle that could only read
//! `assets/stars.cat` would skip wherever nobody has cooked one, CI included,
//! and a check that mostly skips is the sort that reports success from an
//! empty run.

use std::path::PathBuf;

use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::shot;
use engine::stars::{Catalogue, Star};

use game::app;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// A catalogue on disk, at a path of this test's own.
fn write_catalogue(name: &str, stars: Vec<Star>) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, Catalogue { stars }.to_bytes()).expect("the catalogue should write");
    path
}

fn a_star() -> Star {
    Star {
        dir: [1.0, 0.0, 0.0],
        magnitude: 1.0,
        colour_index: 0.0,
    }
}

#[test]
fn a_catalogue_on_disk_becomes_a_sky_in_the_frame() {
    let Some(gpu) = gpu() else {
        return;
    };

    let path = write_catalogue("space_sim_stars_ok.cat", vec![a_star(), a_star()]);
    let mut frame = Frame::new(&gpu, shot::FORMAT);
    assert!(!frame.has_stars(), "a fresh frame has no sky yet");

    app::load_stars(&gpu, &mut frame, &path);

    assert!(
        frame.has_stars(),
        "the frame should carry the catalogue the game just read"
    );
}

/// The asset is not in git, so its absence is a state the game reaches on any
/// fresh clone. It must be a black sky, not a dead process.
#[test]
fn a_missing_catalogue_leaves_the_sky_black() {
    let Some(gpu) = gpu() else {
        return;
    };

    let path = std::env::temp_dir().join("space_sim_stars_absent.cat");
    let _ = std::fs::remove_file(&path);

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    app::load_stars(&gpu, &mut frame, &path);

    assert!(
        !frame.has_stars(),
        "a catalogue that is not there cannot have loaded"
    );
}

/// A truncated file is refused by `Catalogue::from_bytes`, and the game has to
/// carry that refusal the same way it carries the absence. Checked separately
/// from the absence because it takes a different branch: the file opens.
#[test]
fn a_damaged_catalogue_leaves_the_sky_black() {
    let Some(gpu) = gpu() else {
        return;
    };

    let path = std::env::temp_dir().join("space_sim_stars_damaged.cat");
    let bytes = Catalogue {
        stars: vec![a_star()],
    }
    .to_bytes();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("the half file should write");

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    app::load_stars(&gpu, &mut frame, &path);

    assert!(!frame.has_stars(), "half a catalogue is not a sky");
}
