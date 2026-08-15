//! Тягнення вузла маневру (ROADMAP-UI.md, U4b).
//!
//! Перевірка кроку дослівно: **тягнення на N пікселів уздовж ручки prograde
//! дає зміну Δv, лінійну за N і з правильним знаком; тягнення уздовж normal
//! не чіпає prograde.** Друге твердження ловить переплутані осі базису — ту
//! саму помилку, під яку L4 будував окремий оракул у фізиці.
//!
//! Тут вона ловиться інакше й дешевше: осі VNB на екрані **не ортогональні**,
//! тож розкладання довільного тягнення на всі три відразу міняло б prograde
//! при русі вздовж normal. Ручки роблять вимогу істинною за побудовою — і
//! тест перевіряє, що так воно й лишилось.

use engine::camera::Camera;
use engine::frame::FOV_Y;

use game::node::{self, Grab, NodeOnScreen, GRAB_PX, HANDLE_PX, M_S_PER_PX};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// Вузол із наперед заданими осями — без камери й без снапшоту.
///
/// Осі свідомо не ортогональні: саме так вони й виглядають на екрані після
/// проєкції, і саме на такому вузлі помилка «розкласти на всі три» видна.
fn node() -> NodeOnScreen {
    let diagonal = (0.5f32).sqrt();
    NodeOnScreen {
        index: 0,
        at: [640.0, 360.0],
        axes: [[1.0, 0.0], [diagonal, -diagonal], [0.0, 1.0]],
    }
}

/// Тягнення вздовж ручки лінійне за довжиною й має правильний знак.
#[test]
fn dragging_a_handle_is_linear_in_pixels() {
    let node = node();

    let ten = node::drag_to_delta(&node, 0, [10.0, 0.0]);
    let twenty = node::drag_to_delta(&node, 0, [20.0, 0.0]);
    let back = node::drag_to_delta(&node, 0, [-10.0, 0.0]);

    assert!(
        (ten - 10.0 * M_S_PER_PX).abs() < 1e-9,
        "десять пікселів дали {ten} м/с"
    );
    assert!(
        (twenty - 2.0 * ten).abs() < 1e-9,
        "подвоєне тягнення мало дати подвоєну зміну: {twenty} проти {ten}"
    );
    assert!(
        (back + ten).abs() < 1e-9,
        "тягнення назад мало дати протилежний знак: {back} проти {ten}"
    );
}

/// Рух упоперек ручки не робить нічого.
///
/// Це та половина, без якої «лінійне за N» пройшло б і для тягнення, яке
/// рахує довжину руху, а не його напрямок.
#[test]
fn dragging_across_a_handle_does_nothing() {
    let node = node();
    let across = node::drag_to_delta(&node, 0, [0.0, 25.0]);
    assert!(across.abs() < 1e-9, "упоперек ручки вийшло {across} м/с");
}

/// Схоплена ручка normal міняє **лише** свою компоненту.
///
/// Осі тут неортогональні навмисно: проєкція того самого тягнення на
/// prograde дала б помітне число, і саме його поява означала б, що ручки
/// перестали бути ручками.
#[test]
fn a_normal_handle_never_moves_prograde() {
    let node = node();
    let drag = [30.0, -30.0];

    let mut dv = [0.0f64; 3];
    dv[1] += node::drag_to_delta(&node, 1, drag);

    assert!(dv[1] > 0.0, "normal мав вирости, а вийшло {}", dv[1]);
    assert_eq!(dv[0], 0.0, "prograde зрушив разом із normal");
    assert_eq!(dv[2], 0.0, "outward зрушив разом із normal");

    // І доказ, що перевірка не тавтологічна: те саме тягнення, розкладене
    // проєкцією на prograde, дало б помітне число.
    let leak = node::drag_to_delta(&node, 0, drag);
    assert!(
        leak.abs() > 1.0,
        "перевірка порожня: проєкція на prograde дала б {leak} м/с"
    );
}

/// Хапається найближча ручка, і лише в межах радіуса.
#[test]
fn picking_takes_the_nearest_handle_and_only_nearby() {
    let node = node();
    let nodes = [node];

    // Точно на ручці prograde.
    let on_prograde = node.handle(0);
    assert_eq!(
        node::pick_handle(&nodes, on_prograde),
        Some(Grab { node: 0, axis: 0 })
    );

    // Точно на ручці outward — інша вісь того самого вузла.
    let on_outward = node.handle(2);
    assert_eq!(
        node::pick_handle(&nodes, on_outward),
        Some(Grab { node: 0, axis: 2 })
    );

    // На пів радіуса від ручки — ще хапається.
    let near = [on_prograde[0] + GRAB_PX * 0.5, on_prograde[1]];
    assert_eq!(
        node::pick_handle(&nodes, near),
        Some(Grab { node: 0, axis: 0 })
    );

    // А за радіусом — ні, і це не «нічого не сталося»: без цієї межі клік
    // будь-де на екрані хапав би найближчу ручку й тягнув маневр.
    let far = [on_prograde[0] + GRAB_PX * 3.0, on_prograde[1]];
    assert_eq!(node::pick_handle(&nodes, far), None);
}

/// Вісь, що дивиться в камеру, ручки не має.
///
/// Інакше три ручки злиплися б в одній точці, і вибір між ними став би
/// випадковим — гравець тягнув би не ту вісь, не розуміючи чому.
#[test]
fn an_axis_pointing_at_the_camera_has_no_handle() {
    let mut node = node();
    node.axes[1] = [0.0, 0.0];

    let nodes = [node];
    // Курсор рівно там, де була б вироджена ручка — тобто в самому вузлі.
    assert_eq!(node::pick_handle(&nodes, node.at), None);
}

/// Вузли беруться з порахованих семплів, а не вигадуються.
///
/// Перевірка проєкції як такої вже є в `engine` (`tests/camera.rs`); тут
/// важливе інше: маневр, до якого прогноз ще не дійшов, вузла **не має**.
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
        // Константи Якобі в цій фікстурі немає: вона про вузли й панелі, а
        // не про карту (U6b3).
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

    // Камера дивиться на апарат збоку, з тисячі кілометрів.
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
        "маневр усередині порахованого мав дати вузол"
    );

    // Ручки мусять бути напрямками, а не нулями: вузол без жодної ручки
    // неможливо схопити, і тоді весь крок нічого не робить.
    //
    // Але не всі три: камера тут дивиться вздовж швидкості апарата, тож
    // **prograde вироджується в точку** — і це не вада тесту, а те, заради
    // чого вироджені осі взагалі відсіюються. На реальній геометрії така
    // камера трапляється сама собою.
    let node = nodes[0];
    let usable: Vec<usize> = (0..3).filter(|&a| node.axes[a] != [0.0, 0.0]).collect();
    assert!(
        usable.len() >= 2,
        "вузол дав лише {} придатних ручок",
        usable.len()
    );
    assert_eq!(
        node.axes[0],
        [0.0, 0.0],
        "камера дивиться вздовж швидкості — prograde мав виродитись"
    );

    for axis in usable {
        let handle = node.handle(axis);
        let away = (handle[0] - node.at[0]).hypot(handle[1] - node.at[1]);
        assert!(
            (away - HANDLE_PX).abs() < 0.01,
            "ручка осі {axis} відійшла на {away}, а мала на {HANDLE_PX}"
        );
    }
}
