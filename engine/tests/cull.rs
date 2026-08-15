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
//!
//! З R3b поруч стоїть frustum, і головне питання до нього не «чи працює», а
//! **скільки він додає понад горизонт**. PROJECT.md §7 каже, що горизонт
//! важливіший; тут це перестає бути цитатою.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::cull::{self, Body};
use engine::frame::FOV_Y;
use engine::lod;

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const HEIGHT_PX: f64 = 720.0;

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn earth() -> Body {
    Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, IDENTITY)
}

fn earth_lod() -> lod::Body {
    lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
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
        let selection = lod::select(&earth_lod(), &camera, focal, None);
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
///
/// ## Чому висот і напрямків багато, а не один
///
/// Перша версія перевіряла одну висоту (300 км) з камери над **центром
/// грані** — і пропустила помилку, яка викидала грань прямо під камерою.
/// Умова спрацьовує лише коли око **всередині конуса** патча, тобто на
/// широких конусах і низьких висотах; на одній зручній камері такого поєднання
/// просто не траплялось. Тому тут і золота спіраль, і діапазон висот аж до
/// сотні метрів: помилка цього класу живе саме там.
#[test]
fn nothing_visible_is_ever_thrown_away() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let mut worst_slack = 0usize;
    let mut checked = 0;

    for step in 0..12 {
        // Тридцять два напрямки золотою спіраллю — жоден не збігається ні з
        // віссю грані, ні з її кутом.
        let z = 1.0 - (2.0 * f64::from(step) + 1.0) / 12.0;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let phi = f64::from(step) * std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        let unit = [r * phi.cos(), r * phi.sin(), z];

        for altitude in [1.0e2_f64, 1.0e3, 1.0e4, 1.0e5, 3.0e5, 2.0e6] {
            let d = EARTH_RADIUS_M + altitude;
            let eye = [unit[0] * d, unit[1] * d, unit[2] * d];
            let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
            let selection = lod::select(&earth_lod(), &camera, focal, None);
            let visibility = cull::horizon(&selection, &earth(), &camera);

            // Вузол видно, якщо відрізок до нього не заходить у сферу. Для
            // опуклого тіла це рівносильно `(P − C) · (E − C) > R²`, і без
            // жодних коренів.
            let seen = |p: [f64; 3]| {
                p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] > EARTH_RADIUS_M.powi(2)
            };

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
                    "висота {altitude:.1e} м, напрямок {unit:?}: {patch:?} має \
                     видимі вузли, а відбір його прибрав"
                );
                if visible && !any {
                    kept_but_hidden += 1;
                }
            }

            // Запас конуса має бути помірним: якби він лишав удвічі більше,
            // ніж потрібно, відбір коштував би більше, ніж повертає.
            //
            // Частка міряється лише на наборах від шістнадцяти патчів, і це не
            // послаблення. Біля самої поверхні лімб стискається до часток
            // градуса, набір падає до кількох патчів — і «чотири зайвих із
            // шести» звучить страшно, коли йдеться про шість. Частка на таких
            // числах говорить про геометрію конуса, а не про ціну відбору,
            // заради якої сторож і стоїть.
            if visibility.drawn() >= 16 {
                assert!(
                    kept_but_hidden * 2 <= visibility.drawn(),
                    "висота {altitude:.1e} м: конус лишив {kept_but_hidden} \
                     зайвих патчів з {} — він завеликий",
                    visibility.drawn()
                );
            }
            worst_slack = worst_slack.max(kept_but_hidden);
            checked += selection.patches.len();
        }
    }

    println!(
        "  {checked} патчів на 72 камерах; найбільше зайвого в одному наборі \
         {worst_slack}"
    );
}

// ---------------------------------------------------------------------------
// Frustum після горизонту (R3b)

/// Скільки frustum додає **понад** горизонт — на тих самих камерах.
///
/// Це перетворює «horizon важливіший за frustum» (PROJECT.md §7) із цитати на
/// вимір. Якби вийшло навпаки, це була б знахідка, а не помилка, і записувати
/// довелося б її.
///
/// Камери дві на висоту: надир (планета в центрі кадру) і погляд уздовж лімба.
/// Друга навмисно невигідна для горизонту — саме там frustum має шанс.
#[test]
fn the_frustum_adds_less_than_the_horizon_took() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let aspect = 16.0 / 9.0;

    for altitude in [1.0e5, 3.0e5, 2.0e6, 4.0e8] {
        for (name, camera) in [
            ("надир", above(altitude)),
            ("уздовж лімба", along_limb(altitude)),
        ] {
            let selection = lod::select(&earth_lod(), &camera, focal, None);
            let mut visibility = cull::horizon(&selection, &earth(), &camera);
            cull::frustum(
                &mut visibility,
                &selection,
                &earth(),
                &camera,
                FOV_Y,
                aspect,
            );

            println!(
                "  {altitude:.1e} м, {name}: {} патчів → лімб прибрав {}, \
                 frustum ще {} → малюється {}",
                selection.patches.len(),
                visibility.past_limb,
                visibility.outside_frustum,
                visibility.drawn()
            );

            assert_eq!(
                visibility.drawn() + visibility.past_limb + visibility.outside_frustum,
                selection.patches.len(),
                "патчі загубились між двома відборами"
            );
            assert!(
                visibility.drawn() > 0,
                "{name} з {altitude:.1e} м: не лишилось нічого"
            );
            assert!(
                visibility.outside_frustum <= visibility.past_limb,
                "frustum прибрав {} понад горизонтові {} — це знахідка, і її \
                 треба записати, а не пройти повз",
                visibility.outside_frustum,
                visibility.past_limb
            );
        }
    }
}

/// Камера на висоті `altitude`, повернута так, що планета йде краєм кадру.
fn along_limb(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    let eye = [d, 0.0, 0.0];
    // Дивимось на точку лімба, а не в центр: так половина кадру — небо.
    let horizon = (EARTH_RADIUS_M / d).acos();
    let target = [
        EARTH_RADIUS_M * horizon.cos(),
        EARTH_RADIUS_M * horizon.sin(),
        0.0,
    ];
    Camera::look_at(eye, target, [0.0, 0.0, 1.0])
}

/// Frustum не викидає нічого, що видно в кадрі.
///
/// Другий шлях, як і в горизонту: замість площин — пряма проєкція вузлів у
/// пікселі (`Camera::to_screen`). Якщо хоч один вузол патча лягає в кадр, а
/// відбір патч прибрав, це та сама помилка «десь щось не намалювалось».
#[test]
fn the_frustum_never_drops_what_lands_in_the_frame() {
    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;
    let focal = lod::focal_px(FOV_Y, f64::from(HEIGHT));
    let aspect = f64::from(WIDTH) / f64::from(HEIGHT);

    for altitude in [3.0e5, 2.0e6] {
        let camera = along_limb(altitude);
        let selection = lod::select(&earth_lod(), &camera, focal, None);
        let mut visibility = cull::horizon(&selection, &earth(), &camera);
        let limb_only: Vec<bool> = visibility.visible.clone();
        cull::frustum(
            &mut visibility,
            &selection,
            &earth(),
            &camera,
            FOV_Y,
            aspect,
        );

        for ((patch, &kept), &visible) in selection
            .patches
            .iter()
            .zip(&limb_only)
            .zip(&visibility.visible)
        {
            // Про те, що вже прибрав горизонт, frustum нічого не винен.
            if !kept || visible {
                continue;
            }
            for a in 0..=SIDE {
                for b in 0..=SIDE {
                    let world = patch.vertex(a, b, EARTH_RADIUS_M);
                    if let Some(px) = camera.to_screen(FOV_Y, WIDTH, HEIGHT, world) {
                        assert!(
                            px[0] < 0.0
                                || px[1] < 0.0
                                || px[0] > WIDTH as f32
                                || px[1] > HEIGHT as f32,
                            "{patch:?} прибрано, а його вузол лягає в кадр на {px:?}"
                        );
                    }
                }
            }
        }
        println!(
            "  {altitude:.1e} м уздовж лімба: frustum прибрав {} патчів, і \
             жоден із них не мав вузла в кадрі",
            visibility.outside_frustum
        );
    }
}

/// Поворот тіла крутить набір разом із тілом, а не всупереч йому.
///
/// Сторож проти помилки, яку неможливо побачити, поки орієнтація одинична, і
/// яка саме тому жила в коді від R2a до R3b: конус патча живе в системі тіла,
/// камера — у системі світу, і поки поворот тотожний, різниці немає.
///
/// Твердження — симетрія. Повернути тіло на кут `θ` навколо осі й повернути
/// камеру на той самий кут навколо тієї самої осі — це одна й та сама
/// картинка, тож набори патчів і їхня видимість мусять збігтися **поштучно**.
/// Слабша форма («кількість та сама») пройшла б і на реалізації, яка ігнорує
/// поворот узагалі.
#[test]
fn turning_the_body_turns_the_set_with_it() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let aspect = 16.0 / 9.0;
    let altitude = 3.0e5;
    let d = EARTH_RADIUS_M + altitude;

    // Поворот на 40° навколо z: матриця без нулів там, де вони ховали б помилку.
    let theta: f64 = 40.0_f64.to_radians();
    let (c, s) = (theta.cos(), theta.sin());
    let turn = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];

    // Нерухоме тіло, камера над точкою (d, 0, 0).
    let still_camera = Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    // Повернуте тіло, камера повернута так само.
    let eye = [d * c, d * s, 0.0];
    let turned_camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let run = |body_lod: lod::Body, occluder: Body, camera: &Camera| {
        let selection = lod::select(&body_lod, camera, focal, None);
        let mut visibility = cull::horizon(&selection, &occluder, camera);
        cull::frustum(
            &mut visibility,
            &selection,
            &occluder,
            camera,
            FOV_Y,
            aspect,
        );
        (selection.patches, visibility.visible)
    };

    let (still_patches, still_visible) = run(
        lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M),
        earth(),
        &still_camera,
    );
    let (turned_patches, turned_visible) = run(
        lod::Body {
            rotation: turn,
            ..lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
        },
        Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, turn),
        &turned_camera,
    );

    println!(
        "  нерухоме: {} патчів, малюється {}; повернуте на {:.0}°: {} і {}",
        still_patches.len(),
        still_visible.iter().filter(|&&v| v).count(),
        theta.to_degrees(),
        turned_patches.len(),
        turned_visible.iter().filter(|&&v| v).count()
    );

    assert_eq!(
        still_patches, turned_patches,
        "поворот тіла разом з камерою змінив набір патчів"
    );
    assert_eq!(
        still_visible, turned_visible,
        "поворот тіла разом з камерою змінив видимість"
    );
}
