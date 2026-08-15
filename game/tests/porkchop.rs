//! Сітка вікон рахується в нитці планувальника (ROADMAP-UI.md, U5b).
//!
//! Три твердження, і жодне з них не про пікселі:
//!
//! 1. сітка з нитки — та сама, що прямий виклик межі, клітинка в клітинку;
//! 2. там, де розв'язку немає, лишається **дірка**, а не нуль;
//! 3. нитка від сітки не глухне: правило скасування в неї одне на два види
//!    роботи, і сітка ним не виламується.
//!
//! Перше з них — про осі. `t1` і `tof` обидва додатні й обидва в секундах, тож
//! транспонована сітка виглядає цілком правдоподібно; U5a ловив це на межі,
//! тут те саме ловиться на щільній сітці, де клітинку ще треба покласти в
//! правильний рядок.

use std::sync::Arc;
use std::time::{Duration, Instant};

use game::mission;
use game::planner::{Planner, PreviewRequest, Request};
use game::porkchop::{Grid, GridRequest};
use game::world::{EARTH, MOON};

const DAY: f64 = 86400.0;
const PATIENCE: Duration = Duration::from_secs(20);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "не дочекалися: {what}");
        std::thread::yield_now();
    }
}

fn ephemeris() -> Arc<core_rs::Ephemeris> {
    Arc::new(core_rs::Ephemeris::load(&mission::default_asset()).expect("фікстура"))
}

/// Сітка, що цілком лежить у проміжку ассета: 120 діб від J2000, тобто
/// відхід до 60-ї доби плюс переліт до 10 діб — із запасом усередині.
fn inside(id: u64, mu: f64) -> GridRequest {
    GridRequest {
        id,
        depart_body: EARTH,
        arrive_body: MOON,
        mu,
        prograde: true,
        t1: (0..40).map(|i| f64::from(i) * 1.5 * DAY).collect(),
        tof: (0..30).map(|i| (1.0 + f64::from(i) * 0.3) * DAY).collect(),
    }
}

fn ask_for(planner: &Planner, request: &GridRequest) -> Grid {
    planner.request(Request::Grid(request.clone()));
    let mut got = None;
    wait_until("сітка", || {
        if let Some(grid) = planner.latest_grid() {
            got = Some(grid);
        }
        got.as_ref().is_some_and(|g: &Grid| g.id == request.id)
    });
    got.expect("щойно перевірили")
}

/// Клітинка з нитки — та сама, що з прямого виклику межі, і в тому самому місці.
///
/// Бітово, і це чесно: обидва шляхи кличуть **ту саму** функцію C з тими
/// самими аргументами (U5a вже звірив її з `lambert_solve` окремо, і там
/// допуск був потрібен). Отже все, що ця перевірка може спіймати, — це
/// розкладання пласкої відповіді C по рядках і стовпцях, а воно або точне,
/// або зовсім не те.
#[test]
fn a_grid_from_the_thread_is_the_boundary_call_laid_out_in_rows() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    assert!(mu > 0.0, "фікстура мусить знати масу Землі");

    let request = inside(1, mu);
    let planner = Planner::spawn(eph.clone(), mission::config()).expect("планувальник");

    let started = Instant::now();
    let grid = ask_for(&planner, &request);
    let took = started.elapsed();

    assert_eq!(grid.cells.len(), request.t1.len() * request.tof.len());
    assert_eq!(grid.t1, request.t1);
    assert_eq!(grid.tof, request.tof);

    let direct = core_rs::porkchop(&eph, EARTH, MOON, mu, true, &request.t1, &request.tof)
        .expect("прямий виклик межі");

    let mut checked = 0;
    for cell in &direct {
        let i = request
            .t1
            .iter()
            .position(|t| *t == cell.t1)
            .expect("t1 клітинки мусить бути на осі");
        let j = request
            .tof
            .iter()
            .position(|t| *t == cell.tof)
            .expect("tof клітинки мусить бути на осі");

        let ours = grid
            .at(i, j)
            .unwrap_or_else(|| panic!("клітинка ({i}, {j}) зійшлася напряму, а в сітці її немає"));
        assert_eq!(
            ours.v_inf_depart.to_bits(),
            cell.v_inf_depart.to_bits(),
            "відхід у ({i}, {j})"
        );
        assert_eq!(
            ours.v_inf_arrive.to_bits(),
            cell.v_inf_arrive.to_bits(),
            "прихід у ({i}, {j})"
        );
        checked += 1;
    }

    // Стільки ж клітинок, скільки й у прямого виклику: інакше сітка десь
    // домалювала те, чого межа не рахувала.
    assert_eq!(
        grid.cells.iter().flatten().count(),
        direct.len(),
        "у сітці інша кількість клітинок, ніж віддала межа"
    );

    let (low, high) = grid.range().expect("сітка, де нічого не зійшлося");
    let (i, j, best) = grid.best().expect("найкраще вікно");
    println!(
        "  {checked} клітинок за {took:?}; ціна від {low:.0} до {high:.0} м/с;\n  \
         найдешевше: відхід на добі {:.1}, переліт {:.1} доби, {:.0} м/с",
        request.t1[i] / DAY,
        request.tof[j] / DAY,
        best.total()
    );

    // Найкраще вікно — справді найдешевше з усіх, а не перше-ліпше.
    for cell in grid.cells.iter().flatten() {
        assert!(cell.total() >= best.total());
    }
    assert!(
        (low - best.total()).abs() < 1e-9,
        "межа шкали й мінімум різні"
    );
}

/// За краєм ассета клітинка **зникає**, а не коштує нуль.
///
/// Це та сама різниця, заради якої сітка щільна: нуль — найдешевший переліт
/// із можливих, тобто на плоті він виглядав би найкращим вікном, і гравець
/// клікнув би саме туди. Фікстура покриває 120 діб, тож переліт, що
/// приземляється пізніше, ефемериді нема з чого порахувати.
#[test]
fn a_window_past_the_end_of_the_asset_is_a_hole_not_a_bargain() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    // Відхід на 115-й добі, переліт від доби до дванадцяти: перші стовпці ще
    // всередині 120 діб, останні — вже за краєм.
    let request = GridRequest {
        id: 7,
        depart_body: EARTH,
        arrive_body: MOON,
        mu,
        prograde: true,
        t1: vec![115.0 * DAY],
        tof: (1..=12).map(|i| f64::from(i) * DAY).collect(),
    };

    let grid = ask_for(&planner, &request);

    let inside = grid.at(0, 0).expect("переліт на добу ще влазить у 120 діб");
    assert!(
        inside.total() > 0.0,
        "клітинка всередині проміжку не може коштувати нуль"
    );
    assert_eq!(
        grid.at(0, 11),
        None,
        "переліт до 127-ї доби — за краєм ассета, а сітка щось про нього знає"
    );

    let holes = grid.cells.iter().filter(|c| c.is_none()).count();
    println!("  {holes} дірок із {} клітинок", grid.cells.len());
    assert!(holes > 0, "заборонених зон не видно — перевіряти нема чого");

    // І найкраще вікно шукається серед того, що є, а не серед дірок.
    let (_, j, _) = grid.best().expect("хоч одне вікно");
    assert!(
        grid.at(0, j).is_some(),
        "найкращим вікном названо дірку — саме цього щільна сітка й не дозволяє"
    );
}

/// Сітка не виламує правила скасування, спільного на два види роботи.
///
/// Порожні осі — це запит ні про що, і відповіді на нього немає (нитка не
/// вигадує порожній плот). Перевірити «нічого не прийшло» можна лише через те,
/// що прийшло далі: якби нитка на такому запиті глухла, наступна відповідь не
/// прийшла б ніколи.
#[test]
fn an_empty_axis_leaves_the_thread_working() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    planner.request(Request::Grid(GridRequest {
        id: 1,
        depart_body: EARTH,
        arrive_body: MOON,
        mu,
        prograde: true,
        t1: Vec::new(),
        tof: vec![DAY],
    }));

    let grid = ask_for(&planner, &inside(2, mu));
    assert_eq!(grid.id, 2);
    assert!(grid.cells.iter().flatten().count() > 0);
}

/// Прев'ю після сітки доходить — і навпаки.
///
/// Два види роботи йдуть одним каналом саме для цього: правило «новіше
/// скасовує старіше» лишається одне, і жоден вид не має власної черги, у якій
/// можна застрягти.
#[test]
fn a_preview_asked_after_a_grid_still_arrives() {
    let sim = game::sim::Sim::spawn(mission::world(&mission::default_asset()).expect("світ"))
        .expect("нитка симуляції");
    sim.send(game::sim::Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("горизонт", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = game::leg::restart_at(&vessel.legs, vessel.start, burn_t);

    let eph = sim.ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("планувальник");

    let mut plan = game::plan::Plan::new();
    plan.insert(game::plan::Manoeuvre {
        t: burn_t,
        dv: [-8.0, 0.0, 0.0],
        frame: game::plan::Frame::Vnb { body: EARTH },
    });

    planner.request(Request::Grid(inside(1, mu)));
    planner.request(Request::Preview(PreviewRequest {
        id: 2,
        vessel: vessel.id,
        from: restart.state,
        step: restart.step,
        plan,
        params: vessel.params,
        horizon_end: vessel.horizon_end,
    }));

    let mut preview = None;
    wait_until("прев'ю після сітки", || {
        if let Some(got) = planner.latest() {
            preview = Some(got);
        }
        preview.as_ref().is_some_and(|p| p.id == 2)
    });
    assert!(!preview.expect("щойно перевірили").legs.is_empty());
}

// ---------------------------------------------------------------------------
// Плот: зображення, осі, курсор (U5c)
//
// Ані ассета, ані нитки тут уже немає — сітка збирається руками. Так і
// задумано: усе нижче перевіряє переклад сітки в екран, а він не має права
// залежати від того, звідки сітка взялася.

use engine::egui;
use game::hud;
use game::porkchop::{cell_at, colour, Cell};
use game::text::Language;

/// Сітка 4×3 з дірою в кутку: ціни ростуть зі збільшенням обох індексів.
fn handmade() -> Grid {
    let t1: Vec<f64> = (0..4).map(|i| f64::from(i) * DAY).collect();
    let tof: Vec<f64> = (1..4).map(|j| f64::from(j) * DAY).collect();

    let mut cells = Vec::new();
    for i in 0..t1.len() {
        for j in 0..tof.len() {
            // Правий верхній кут — заборонена зона.
            cells.push(if i == 3 && j == 2 {
                None
            } else {
                Some(Cell {
                    v_inf_depart: 100.0 * (i + 1) as f64,
                    v_inf_arrive: 10.0 * (j + 1) as f64,
                })
            });
        }
    }

    Grid {
        id: 42,
        t1,
        tof,
        cells,
    }
}

/// Дірка прозора, ціна — ні, і дешеве не схоже на дороге.
///
/// Це три властивості кольору, від яких залежить, чи можна плоту вірити.
/// Найважливіша — перша: непрозора дірка лягла б на ту саму шкалу, що й ціни,
/// і око почало б порівнювати її з ними.
#[test]
fn a_hole_is_transparent_and_a_price_is_not() {
    let cheap = Cell {
        v_inf_depart: 100.0,
        v_inf_arrive: 10.0,
    };
    let costly = Cell {
        v_inf_depart: 900.0,
        v_inf_arrive: 90.0,
    };
    let (low, high) = (cheap.total(), costly.total());

    assert_eq!(colour(None, low, high)[3], 0, "дірка мусить бути прозорою");
    assert_eq!(colour(Some(cheap), low, high)[3], 255);
    assert_eq!(colour(Some(costly), low, high)[3], 255);
    assert_ne!(
        colour(Some(cheap), low, high),
        colour(Some(costly), low, high),
        "кінці шкали пофарбовані однаково — плот нічого не показує"
    );

    // Уся сітка однакова — це дешевий кінець, а не дорогий і не ділення на нуль.
    let flat = colour(Some(cheap), low, low);
    assert_eq!(flat, colour(Some(cheap), low, high));
    assert_eq!(flat[3], 255);
}

/// Шкала монотонна: дорожче — не «інакше», а далі в один бік.
#[test]
fn the_scale_goes_one_way() {
    let (low, high) = (100.0, 1000.0);
    let mut previous = colour(
        Some(Cell {
            v_inf_depart: low,
            v_inf_arrive: 0.0,
        }),
        low,
        high,
    );

    for step in 1..=9 {
        let cell = Cell {
            v_inf_depart: low + f64::from(step) * 100.0,
            v_inf_arrive: 0.0,
        };
        let now = colour(Some(cell), low, high);
        assert!(
            now[0] >= previous[0] && now[2] <= previous[2],
            "на кроці {step} шкала повернула назад: {previous:?} → {now:?}"
        );
        previous = now;
    }
}

/// Низ плоту — найкоротший переліт, і саме тут ламається переворот осі.
///
/// Зображення йде рядками згори вниз, а `tof` на плоті росте вгору. Забути
/// цей переворот легко, а виглядає забуття як цілком правдоподібний плот, у
/// якому курсор просто відповідає дзеркально.
#[test]
fn the_bottom_of_the_plot_is_the_shortest_flight() {
    let grid = handmade();

    assert_eq!(cell_at(&grid, 0.01, 0.01), Some((0, 0)), "лівий нижній кут");
    assert_eq!(cell_at(&grid, 0.99, 0.99), Some((3, 2)), "правий верхній");
    assert_eq!(
        cell_at(&grid, 0.01, 0.99),
        Some((0, 2)),
        "лівий верхній: перший відхід, найдовший переліт"
    );

    // Поза плотом клітинки немає — інакше промах повз край читався б як
    // вибір крайньої.
    assert_eq!(cell_at(&grid, -0.1, 0.5), None);
    assert_eq!(cell_at(&grid, 0.5, 1.2), None);
}

/// Числа під курсором — числа тієї клітинки, а не сусідньої.
///
/// Панель малюється без вікна: `RawInput` із позицією миші, і те, що вийшло,
/// шукається серед намальованого тексту. Пікселі тут ні до чого — панель із
/// NaN виглядає точнісінько так само, як панель із правильними числами.
#[test]
fn the_readout_shows_the_cell_under_the_cursor() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::porkchop_panel(ui, Language::English, Some(&grid), &mut state);
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .flat_map(|clipped| texts(&clipped.shape))
            .collect()
    };

    // Кадр-розігрів: до першого малювання плот не має ні місця, ні розміру.
    draw(Vec::new());

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("плот мусить бути намальований")
        .rect;

    // Наводимо на клітинку (2, 0): третій відхід, найкоротший переліт.
    let at = egui::pos2(
        rect.min.x + rect.width() * (2.5 / 4.0),
        rect.max.y - rect.height() * (0.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(at)]);
    let all = said.join(" | ");

    let cell = grid.at(2, 0).expect("клітинка (2, 0) не дірка");
    assert!(
        all.contains(&format!(
            "{:.0} / {:.0}",
            cell.v_inf_depart, cell.v_inf_arrive
        )),
        "серед намальованого немає чисел клітинки (2, 0): {all}"
    );
    assert!(
        all.contains("1.00 days"),
        "переліт клітинки (2, 0) — доба, а панель каже: {all}"
    );

    // А тепер дірка — і вона мусить назватися діркою, а не мовчати.
    let hole = egui::pos2(
        rect.min.x + rect.width() * (3.5 / 4.0),
        rect.max.y - rect.height() * (2.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(hole)]).join(" | ");
    assert!(
        said.contains(game::text::tr(
            Language::English,
            game::text::Key::NoSolution
        )),
        "заборонена зона нічого не сказала про себе: {said}"
    );
}

/// Клік по плоту обирає вікно — те, на яке дивилися.
#[test]
fn a_click_chooses_the_window_under_the_pointer() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let draw = |state: &mut hud::PlotState, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, Some(&grid), state);
        });
        output.textures_delta.clear();
        actions
    };

    assert_eq!(
        draw(&mut state, Vec::new()),
        Vec::new(),
        "плот сам не клікає"
    );

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("плот мусить бути намальований")
        .rect;
    let at = egui::pos2(
        rect.min.x + rect.width() * (1.5 / 4.0),
        rect.max.y - rect.height() * (1.5 / 3.0),
    );

    let actions = draw(
        &mut state,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );

    assert_eq!(actions, vec![hud::PorkchopAction::Choose(1, 1)]);
    assert_eq!(state.chosen, Some((1, 1)));
}

/// Кнопка просить сітку — і рівно це, без жодного вибору вікна.
#[test]
fn the_button_asks_for_a_grid_and_nothing_else() {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, None, &mut state);
        });
        output.textures_delta.clear();
        actions
    };

    draw(Vec::new());
    let centre = context
        .read_response(egui::Id::new(hud::PLOT_COMPUTE))
        .expect("кнопка мусить бути намальована")
        .rect
        .center();

    let actions = draw(vec![
        egui::Event::PointerMoved(centre),
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);

    assert_eq!(actions, vec![hud::PorkchopAction::Compute]);
}

/// Увесь текст фігури — плаский список рядків.
fn texts(shape: &egui::epaint::Shape) -> Vec<String> {
    match shape {
        egui::epaint::Shape::Text(text) => vec![text.galley.text().to_string()],
        egui::epaint::Shape::Vec(shapes) => shapes.iter().flat_map(texts).collect(),
        _ => Vec::new(),
    }
}
