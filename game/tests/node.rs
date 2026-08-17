//! Dragging a manoeuvre node (ROADMAP-UI.md, U4b).
//!
//! The step verbatim: **a drag of N pixels along the prograde handle changes
//! dv linearly in N and with the right sign; a drag along normal does not
//! touch prograde.** The second claim catches swapped basis axes -- the very
//! bug L4 built a separate oracle for down in the physics.
//!
//! Here it is caught more cheaply: the VNB axes on screen are **not
//! orthogonal**, so decomposing an arbitrary drag onto all three at once
//! would move prograde when dragging along normal. Handles make the
//! requirement true by construction, and the test checks it stayed that way.

use engine::camera::Camera;
use engine::frame::FOV_Y;

use game::node::{self, Grab, NodeOnScreen, GRAB_PX, HANDLE_PX, M_S_PER_PX};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// A node with axes given up front -- no camera and no snapshot.
///
/// The axes are deliberately not orthogonal: that is how they look on screen
/// after projection, and it is on such a node that "decompose onto all three"
/// shows up.
fn node() -> NodeOnScreen {
    let diagonal = (0.5f32).sqrt();
    NodeOnScreen {
        index: 0,
        at: [640.0, 360.0],
        axes: [[1.0, 0.0], [diagonal, -diagonal], [0.0, 1.0]],
    }
}

/// Dragging a handle is linear in length and has the right sign.
#[test]
fn dragging_a_handle_is_linear_in_pixels() {
    let node = node();

    let ten = node::drag_to_delta(&node, 0, [10.0, 0.0]);
    let twenty = node::drag_to_delta(&node, 0, [20.0, 0.0]);
    let back = node::drag_to_delta(&node, 0, [-10.0, 0.0]);

    assert!(
        (ten - 10.0 * M_S_PER_PX).abs() < 1e-9,
        "ten pixels gave {ten} m/s"
    );
    assert!(
        (twenty - 2.0 * ten).abs() < 1e-9,
        "a doubled drag should have doubled the change: {twenty} against {ten}"
    );
    assert!(
        (back + ten).abs() < 1e-9,
        "dragging back should have flipped the sign: {back} against {ten}"
    );
}

/// Moving across a handle does nothing.
///
/// This is the half without which "linear in N" would also pass for a drag
/// that measures the length of the motion rather than its direction.
#[test]
fn dragging_across_a_handle_does_nothing() {
    let node = node();
    let across = node::drag_to_delta(&node, 0, [0.0, 25.0]);
    assert!(across.abs() < 1e-9, "across the handle gave {across} m/s");
}

/// A grabbed normal handle changes **only** its own component.
///
/// The axes are non-orthogonal on purpose: projecting the same drag onto
/// prograde would give a noticeable number, and that number appearing is what
/// would mean the handles stopped being handles.
#[test]
fn a_normal_handle_never_moves_prograde() {
    let node = node();
    let drag = [30.0, -30.0];

    let mut dv = [0.0f64; 3];
    dv[1] += node::drag_to_delta(&node, 1, drag);

    assert!(dv[1] > 0.0, "normal should have grown, but gave {}", dv[1]);
    assert_eq!(dv[0], 0.0, "prograde moved along with normal");
    assert_eq!(dv[2], 0.0, "outward moved along with normal");

    // And proof the check is not a tautology: the same drag, projected onto
    // prograde, would give a noticeable number.
    let leak = node::drag_to_delta(&node, 0, drag);
    assert!(
        leak.abs() > 1.0,
        "the check is empty: projection onto prograde would give {leak} m/s"
    );
}

/// The nearest handle is picked, and only within the radius.
#[test]
fn picking_takes_the_nearest_handle_and_only_nearby() {
    let node = node();
    let nodes = [node];

    // Exactly on the prograde handle.
    let on_prograde = node.handle(0);
    assert_eq!(
        node::pick_handle(&nodes, on_prograde),
        Some(Grab { node: 0, axis: 0 })
    );

    // Exactly on the outward handle -- another axis of the same node.
    let on_outward = node.handle(2);
    assert_eq!(
        node::pick_handle(&nodes, on_outward),
        Some(Grab { node: 0, axis: 2 })
    );

    // Half a radius away -- still grabbed.
    let near = [on_prograde[0] + GRAB_PX * 0.5, on_prograde[1]];
    assert_eq!(
        node::pick_handle(&nodes, near),
        Some(Grab { node: 0, axis: 0 })
    );

    // Beyond the radius, no -- and this is not "nothing happened": without the
    // bound a click anywhere on screen would grab the nearest handle and drag
    // the manoeuvre.
    let far = [on_prograde[0] + GRAB_PX * 3.0, on_prograde[1]];
    assert_eq!(node::pick_handle(&nodes, far), None);
}

/// An axis pointing at the camera has no handle.
///
/// Otherwise the three handles would collapse into one point and the choice
/// between them would be arbitrary -- the player would drag the wrong axis
/// without understanding why.
#[test]
fn an_axis_pointing_at_the_camera_has_no_handle() {
    let mut node = node();
    node.axes[1] = [0.0, 0.0];

    let nodes = [node];
    // The cursor exactly where the degenerate handle would be, i.e. on the node.
    assert_eq!(node::pick_handle(&nodes, node.at), None);
}

/// Nodes come from computed samples rather than being invented.
///
/// Projection as such is already checked in `engine` (`tests/camera.rs`);
/// what matters here is that a manoeuvre the forecast has not reached yet has
/// **no** node.
#[test]
fn a_manoeuvre_beyond_the_forecast_has_no_node() {
    use core_rs::{State, Stop, Vec3d};
    use game::leg::{Leg, Sample};
    use game::plan::{Frame, Manoeuvre};
    use game::world::{VesselId, EARTH};
    use std::sync::Arc;

    let sample = |t: f64| Sample {
        state: State {
            t,
            r: Vec3d {
                x: 7.0e6,
                y: 0.0,
                z: 0.0,
            },
            v: Vec3d {
                x: 0.0,
                y: 7500.0,
                z: 0.0,
            },
        },
        earth: [0.0; 3],
        moon: [0.0; 3],
    };

    let vessel = game::snapshot::VesselSnapshot {
        // No Jacobi constant in this fixture: it is about nodes and panels,
        // not about the map (U6b3).
        jacobi: None,
        id: VesselId(0),
        name: "probe".to_string(),
        legs: vec![Arc::new(Leg {
            entry: sample(0.0).state,
            t1: 100.0,
            step_out: 1.0,
            samples: vec![sample(0.0), sample(50.0), sample(100.0)],
            stop: Stop::BufferFull,
        })],
        state: sample(0.0).state,
        plan: game::plan::Plan::new(),
        start: sample(0.0).state,
        tip: sample(100.0).state,
        computed_to: 100.0,
        horizon_end: 1.0e6,
        params: None,
        failed: None,
    };

    // The camera looks at the vessel from the side, a thousand kilometres out.
    let camera = Camera::look_at([7.0e6, -1.0e6, 0.0], [7.0e6, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let inside = Manoeuvre {
        t: 50.0,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Vnb { body: EARTH },
    };
    let nodes = node::nodes_on_screen(&camera, FOV_Y, WIDTH, HEIGHT, &vessel, &[inside]);
    assert_eq!(
        nodes.len(),
        1,
        "a manoeuvre inside the computed span should have given a node"
    );

    // The handles must be directions rather than zeros: a node with no handle
    // at all cannot be grabbed, and then the whole step does nothing.
    //
    // But not all three: the camera here looks along the vessel's velocity, so
    // **prograde degenerates to a point** -- not a flaw of the test but the
    // very reason degenerate axes are filtered out. On real geometry such a
    // camera happens by itself.
    let node = nodes[0];
    let usable: Vec<usize> = (0..3).filter(|&a| node.axes[a] != [0.0, 0.0]).collect();
    assert!(
        usable.len() >= 2,
        "the node gave only {} usable handles",
        usable.len()
    );
    assert_eq!(
        node.axes[0],
        [0.0, 0.0],
        "the camera looks along the velocity -- prograde should have degenerated"
    );

    for axis in usable {
        let handle = node.handle(axis);
        let away = (handle[0] - node.at[0]).hypot(handle[1] - node.at[1]);
        assert!(
            (away - HANDLE_PX).abs() < 0.01,
            "the handle of axis {axis} went out {away}, but should have gone {HANDLE_PX}"
        );
    }
}
