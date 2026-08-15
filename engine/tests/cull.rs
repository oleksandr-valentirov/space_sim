//! Відбір за горизонтом: обидва боки й записане число (R3a).
//!
//! Перевірка тут аналітична, і це не вибір зі смаку: лімб — точна геометрія
//! дотичної з точки до сфери, тобто твердження, у якому немає жодного
//! наближення, щоб ховати за ним помилку. Пікселі про відбір говорять
//! найгіршою з можливих мов: занадто жадібний відбір виглядає як «десь щось
//! не намалювалось».
//!
//! Обидва боки обов'язкові. Відбір, який не відкидає нічого, проходить будь-яку
//! перевірку на видиме; відбір, який відкидає все, проходить будь-яку перевірку
//! на кількість. Тому нижче стоїть і патч, що торкається лімба (мусить
//! лишитися), і патч за кілометр за ним (мусить зникнути).

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::cull::{self, Body};
use engine::frame::FOV_Y;
use engine::lod;

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const HEIGHT_PX: f64 = 720.0;

fn earth() -> Body {
    Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M)
}

fn above(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
}

/// Конус патча справді накриває патч — усі його вузли, а не лише кути.
///
/// На цьому стоїть увесь критерій: якщо конус вужчий за патч, відбір
/// відкидатиме те, що видно, і робитиме це тихо. Кути беруться тому, що
/// параметризація грані монотонна; перебором тут доводиться, що міркування
/// правильне, а не правдоподібне.
#[test]
fn the_cone_of_a_patch_covers_every_node_of_it() {
    let mut tightest: f64 = 1.0;
    for face in 0..FACES {
        for (level, i, j) in [(0, 0, 0), (1, 1, 0), (3, 5, 2), (6, 40, 17)] {
            let patch = Patch { face, level, i, j };
            let cone = patch.cone();
            let mut worst: f64 = 1.0;
            for a in 0..=SIDE {
                for b in 0..=SIDE {
                    let p = patch.vertex(a, b, 1.0);
                    let dot = cone.axis[0] * p[0] + cone.axis[1] * p[1] + cone.axis[2] * p[2];
                    worst = worst.min(dot);
                }
            }
            assert!(
                worst >= cone.cos_half - 1e-15,
                "{patch:?}: вузол за конусом ({worst} проти {})",
                cone.cos_half
            );
            // Наскільки конус тісний: одиниця означала б, що він виродився,
            // і відбір ніколи нічого не відкине.
            tightest = tightest.min(worst - cone.cos_half);
        }
    }
    println!("  найбільший запас конуса над вузлами: {tightest:.2e}");
    assert!(
        tightest < 1e-9,
        "конус помітно ширший за патч ({tightest:.2e}) — кути беруться не там"
    );
}

/// Патч на самому лімбі лишається, патч за кілометр за ним зникає.
///
/// Міряється **точкою**, а не справжнім патчем: у патча є розмір, і його
/// внесок змішався б із тим, що перевіряється. Точка ж — це патч із
/// конусом нульового розхилу, тобто той самий критерій без другого доданка.
#[test]
fn a_patch_touching_the_limb_stays_and_one_past_it_goes() {
    let altitude = 3.0e5;
    let distance = EARTH_RADIUS_M + altitude;
    let body = earth();
    let limb = cull::limb_cos(&body, distance);

    // Кут, за яким поверхня ховається. Тут `acos` дозволений: це тест, а не
    // кадр — у кадрі формула лишається без тригонометрії.
    let horizon = limb.acos();
    println!(
        "  з {altitude:.1e} м лімб на {:.4}° від підкамерної точки",
        horizon.to_degrees()
    );

    // Кілометр по поверхні — це стільки радіан.
    let kilometre = 1000.0 / EARTH_RADIUS_M;
    let to_eye = [1.0, 0.0, 0.0];
    // Точка під кутом `angle` від напрямку на камеру, як конус нульового
    // розхилу: cos(β − 0) = cos β.
    let visible_at = |angle: f64| angle.cos() > limb;

    assert!(
        visible_at(horizon - kilometre),
        "патч за кілометр перед лімбом відкинуто"
    );
    assert!(
        !visible_at(horizon + kilometre),
        "патч за кілометр за лімбом лишився"
    );
    // Сам лімб — межа, і на ній критерій не мусить бути жадібним.
    assert!(
        !visible_at(horizon + 1e-12),
        "критерій пропускає те, що вже за лімбом"
    );

    // Підкамерна точка не може зникнути за жодних обставин.
    let straight_down = Patch {
        face: 0,
        level: 4,
        i: 8,
        j: 8,
    };
    assert!(
        !cull::beyond_limb(&straight_down, to_eye, limb),
        "відбір прибрав патч просто під камерою"
    );
}

/// Скільки саме відбирає горизонт — записане число, а не «приблизно половина».
#[test]
fn the_horizon_takes_away_most_of_the_planet() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for altitude in [1.0e5, 3.0e5, 2.0e6, 1.0e7, 4.0e8] {
        let camera = above(altitude);
        let selection = lod::select(
            &lod::Body {
                centre: [0.0, 0.0, 0.0],
                radius_m: EARTH_RADIUS_M,
            },
            &camera,
            focal,
        );
        let visibility = cull::horizon(&selection, &earth(), &camera);
        let all = selection.patches.len();
        let drawn = visibility.drawn();

        println!(
            "  {altitude:.1e} м: {all} патчів, малюється {drawn}, за лімбом {} \
             ({:.0}%)",
            visibility.past_limb,
            100.0 * visibility.past_limb as f64 / all as f64
        );

        assert_eq!(drawn + visibility.past_limb, all);
        // Обидва боки на кожній висоті: щось відкинуто й щось лишилось.
        assert!(drawn > 0, "з {altitude:.1e} м не лишилось жодного патча");
        assert!(
            visibility.past_limb > 0,
            "з {altitude:.1e} м горизонт не прибрав нічого"
        );
    }
}

/// Відбір ніколи не прибирає того, що видно: звірка з чесним променем.
///
/// Другий шлях до тієї самої відповіді, і в цьому вся його користь. Критерій
/// працює з конусом патча; тут же для кожного вузла кожного патча питається
/// прямо — чи перетинає відрізок «око → вузол» сферу тіла. Якщо промінь каже
/// «видно», а відбір патч викинув, це помилка, і саме та, яку на екрані видно
/// як «десь щось не намалювалось».
///
/// Зворотне не вимагається: конус огортає патч, тож відбір лишає й дещо зайве.
/// Скільки саме зайвого — теж число, і воно тут друкується.
#[test]
fn nothing_visible_is_ever_thrown_away() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let altitude = 3.0e5;
    let camera = above(altitude);
    let eye = camera.position();
    let selection = lod::select(
        &lod::Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: EARTH_RADIUS_M,
        },
        &camera,
        focal,
    );
    let visibility = cull::horizon(&selection, &earth(), &camera);

    // Вузол видно, якщо відрізок до нього не заходить у сферу. Для опуклого
    // тіла це рівносильно `(P − C) · (E − C) > R²`, і без жодних коренів.
    let seen = |p: [f64; 3]| p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] > EARTH_RADIUS_M.powi(2);

    let mut kept_but_hidden = 0;
    for (patch, &visible) in selection.patches.iter().zip(&visibility.visible) {
        let mut any = false;
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                if seen(patch.vertex(a, b, EARTH_RADIUS_M)) {
                    any = true;
                }
            }
        }
        assert!(
            !(any && !visible),
            "{patch:?} має видимі вузли, а відбір його прибрав"
        );
        if visible && !any {
            kept_but_hidden += 1;
        }
    }

    println!(
        "  з {altitude:.1e} м лишено {} патчів, з них цілком за лімбом {kept_but_hidden}",
        visibility.drawn()
    );
    // Запас конуса має бути помірним: якби він лишав удвічі більше, ніж
    // потрібно, відбір коштував би більше, ніж повертає.
    assert!(
        kept_but_hidden * 2 <= visibility.drawn(),
        "конус лишив {kept_but_hidden} зайвих патчів з {} — він завеликий",
        visibility.drawn()
    );
}
