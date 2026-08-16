//! Сяйво планети на корпус (ROADMAP, T6).
//!
//! PROJECT.md §7 вимагає цього прямо: ambient у кадрі нуль, тож тіньовий бік
//! корабля чорний — і мусить бути чорним, — доки під ним не з'явиться
//! планета. Освітлює його не «трохи світла звідусіль», а конкретне тіло з
//! конкретним альбедо, і саме тому це не константа, а обчислення.
//!
//! ## Диск, а не точка, і саме звідси береться число
//!
//! Планета під кораблем — не точкове джерело: з низької орбіти вона займає
//! майже півсфери. Для ламбертівського диска з півкутом `θ` і сталою
//! яскравістю `L` опромінення площадки, зверненої до його центра, дорівнює
//! `π·L·sin²θ` — це замкнена форма, а не наближення.
//!
//! Яскравість поверхні при одиничному опроміненні від світила — `A·cos/π`, де
//! `A` — альбедо. Разом: `E = A · cos · sin²θ`, і жодного вільного множника в
//! цьому виразі немає.
//!
//! ## Три спрощення, кожне з названою ціною
//!
//! **1. Диск замінює півсферу.** З висоти в сотні кілометрів `θ` доходить до
//! 80°, і диск такого розміру вже помітно не плаский. Ціна — завищене
//! опромінення для площадок, повернутих убік; правильна відповідь потребує
//! інтегрування по видимій шапці, тобто таблиці.
//!
//! **2. Джерело зводиться до напрямку на центр тіла.** Тобто корпус ловить
//! сяйво так, ніби воно приходить з однієї точки. Для дифузного члена це
//! дрібниця, для дзеркального — ні: справжнє відбиття планети в полірованому
//! борту було б диском, а не точкою.
//!
//! **3. Освітленість береться в підкорабельній точці.** Тобто термінатор
//! перетинається миттєво, замість того щоб сповзати по диску. Ціна видима
//! рівно над термінатором і ніде більше.
//!
//! Усі три знімає одна й та сама робота — SH-проба з інтегруванням по шапці,
//! як і написано в PROJECT.md §7. Спільне в них те, що кожне видно лише на
//! **формі** диска, тобто там, де сьогодні нема чого міряти: сяйво входить у
//! кадр одним напрямком і трьома числами.
//!
//! А от **альбедо під кораблем більше не спрощене**: тіло з колірним
//! тайлсетом віддає відлік асета (`Colour::under`, T6c), тобто над морем
//! корабель підсвічений слабше, ніж над материком, і це виміряне число, а не
//! правдоподібність.

use crate::scene::{Body, Scene};

/// Що планета світить на корабель.
pub struct Shine {
    /// Напрямок **до джерела**, тобто вниз, на центр тіла; світові осі.
    pub direction: [f64; 3],
    /// Опромінення по каналах, у тих самих одиницях, що й світило (одиниця).
    pub irradiance: [f64; 3],
}

impl Shine {
    /// Порожнє сяйво — нікуди й нуль.
    pub fn none() -> Shine {
        Shine {
            direction: [0.0, 0.0, 1.0],
            irradiance: [0.0; 3],
        }
    }
}

/// Скільки світить тіло `body` на точку `point` своїм кольором.
///
/// Для тіла з колірним тайлсетом правильна відповідь інша — там альбедо
/// різне в різних місцях, і його бере [`from_body_albedo`]. Кадр викликає
/// саме її, бо тайлсет лежить у нього; ця лишається для тіл без асета й для
/// перевірок самої геометрії.
pub fn from_body(body: &Body, point: [f64; 3], sun: [f64; 3]) -> Shine {
    let albedo = [
        f64::from(body.colour[0]),
        f64::from(body.colour[1]),
        f64::from(body.colour[2]),
    ];
    from_body_albedo(body, point, sun, albedo)
}

/// Те саме, але альбедо поверхні під точкою задане ззовні.
///
/// Розділено рівно тому, що **альбедо в кадрі береться з двох різних місць**:
/// тіло без тайлсета малюється своїм `Body::colour`, тіло з тайлсетом —
/// відліком асета, і `Body::colour` у нього тоді не бере участі взагалі
/// (`surface_albedo` у `patch.slang`). Сяйво мусить нести те саме альбедо,
/// яким пофарбована поверхня, інакше корабель світився б від планети одного
/// кольору над планетою іншого.
///
/// ⚠ Правило матеріалу (`engine::material`) сюди не входить: воно множить
/// яскравість на нахил і шорсткість у межах ±80%, а вибірка тут іде з
/// найгрубішого рівня піраміди, де нахилу такого масштабу вже немає.
pub fn from_body_albedo(body: &Body, point: [f64; 3], sun: [f64; 3], albedo: [f64; 3]) -> Shine {
    let to_centre = [
        body.centre[0] - point[0],
        body.centre[1] - point[1],
        body.centre[2] - point[2],
    ];
    let distance =
        (to_centre[0] * to_centre[0] + to_centre[1] * to_centre[1] + to_centre[2] * to_centre[2])
            .sqrt();
    if distance <= body.radius_m || distance == 0.0 {
        // Усередині тіла сяйва немає — там немає й корабля.
        return Shine::none();
    }
    let down = [
        to_centre[0] / distance,
        to_centre[1] / distance,
        to_centre[2] / distance,
    ];

    // Півкут диска й його форм-фактор.
    let sin_theta = body.radius_m / distance;
    let form = sin_theta * sin_theta;

    // Освітленість підкорабельної точки: її зовнішня нормаль — це `−down`.
    let lit = -(down[0] * sun[0] + down[1] * sun[1] + down[2] * sun[2]);
    let lit = lit.max(0.0);

    Shine {
        direction: down,
        irradiance: [
            albedo[0] * lit * form,
            albedo[1] * lit * form,
            albedo[2] * lit * form,
        ],
    }
}

/// Сяйво від **найближчого** тіла сцени.
///
/// Одне тіло, а не сума, і це не економія: на низькій орбіті найближче тіло
/// закриває півсфери, а решта дає внесок на порядки менший — Земля з Місяця
/// освітлює вчетверо слабше за Місяць з низької орбіти, а Юпітер з Землі
/// не видно взагалі. Сума знадобиться тоді, коли з'явиться сцена з двома
/// тілами на порівнянній відстані.
pub fn nearest(scene: &Scene, point: [f64; 3]) -> Shine {
    match nearest_body(scene, point) {
        Some(k) => from_body(&scene.bodies[k], point, scene.sun),
        None => Shine::none(),
    }
}

/// Яке тіло сцени найближче до точки — індексом, а не посиланням.
///
/// Індекс потрібен саме кадру: тайлсет лежить не в `Body`, а в слоті кадру
/// (`TileSet::Loaded` — це хендл, рушій не знає про формат асета), тож
/// відповідь «ось тіло» ще не дає альбедо. Індексом, а не посиланням, —
/// правило стилю проєкту (CLAUDE.md).
pub fn nearest_body(scene: &Scene, point: [f64; 3]) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (k, body) in scene.bodies.iter().enumerate() {
        let d = [
            body.centre[0] - point[0],
            body.centre[1] - point[1],
            body.centre[2] - point[2],
        ];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - body.radius_m;
        if best.is_none_or(|(previous, _)| distance < previous) {
            best = Some((distance, k));
        }
    }
    best.map(|(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TileSet;

    fn body(colour: [f32; 4]) -> Body {
        Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: 1_000_000.0,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
            colour,
            air: None,
        }
    }

    /// Біля поверхні сяйво прямує до **альбедо**, а не до чогось меншого.
    ///
    /// Це число, а не «яскраво»: на нульовій висоті диск займає півсферу,
    /// `sin θ → 1`, і опромінення дорівнює `A·cos`. Помилка на `π` — найлегша
    /// в цій формулі — робиться тут видимою.
    #[test]
    fn just_above_the_surface_the_shine_is_the_albedo_itself() {
        let planet = body([0.4, 0.5, 0.6, 1.0]);
        let sun = [0.0, 0.0, 1.0];
        // Просто над підсонячною точкою: `cos = 1`.
        let point = [0.0, 0.0, planet.radius_m * 1.000_001];
        let shine = from_body(&planet, point, sun);
        println!("  опромінення {:?}", shine.irradiance);
        for channel in 0..3 {
            assert!(
                (shine.irradiance[channel] - f64::from(planet.colour[channel])).abs() < 1e-4,
                "канал {channel}: {} проти альбедо {}",
                shine.irradiance[channel],
                planet.colour[channel]
            );
        }
    }

    /// З висотою сяйво падає, і падає як `sin²θ`.
    #[test]
    fn the_shine_falls_off_as_the_disc_shrinks() {
        let planet = body([0.5; 4]);
        let sun = [0.0, 0.0, 1.0];
        let mut previous = f64::INFINITY;
        for step in 0..12 {
            let altitude = planet.radius_m * 0.05 * f64::from(1 << step);
            let distance = planet.radius_m + altitude;
            let shine = from_body(&planet, [0.0, 0.0, distance], sun);
            let expected = 0.5 * (planet.radius_m / distance).powi(2);
            assert!(
                (shine.irradiance[0] - expected).abs() < 1e-12,
                "висота {altitude}: {} проти {expected}",
                shine.irradiance[0]
            );
            assert!(shine.irradiance[0] < previous, "сяйво не спало");
            previous = shine.irradiance[0];
        }
    }

    /// Над нічним боком сяйва немає взагалі.
    #[test]
    fn over_the_night_side_there_is_nothing() {
        let planet = body([0.8; 4]);
        let sun = [0.0, 0.0, 1.0];
        let distance = planet.radius_m * 1.2;
        for point in [
            [0.0, 0.0, -distance],
            [0.0, distance * 0.7, -distance * 0.7],
        ] {
            let shine = from_body(&planet, point, sun);
            assert_eq!(shine.irradiance, [0.0; 3], "нічний бік світиться");
        }
        // А точно над термінатором — рівно нуль, не «майже».
        let shine = from_body(&planet, [0.0, distance, 0.0], sun);
        assert_eq!(shine.irradiance, [0.0; 3]);
    }

    /// Колір сяйва — це колір тіла, а не сірий.
    ///
    /// Саме те твердження, яким крок і перевіряється: підсвітка знизу мусить
    /// нести колір поверхні під кораблем.
    #[test]
    fn the_shine_carries_the_colour_of_the_body_below() {
        let sun = [0.0, 0.0, 1.0];
        let point = [0.0, 0.0, 1_200_000.0];
        let blue = from_body(&body([0.2, 0.4, 0.9, 1.0]), point, sun);
        let rust = from_body(&body([0.9, 0.4, 0.2, 1.0]), point, sun);
        println!("  синє {:?}, руде {:?}", blue.irradiance, rust.irradiance);
        assert!(blue.irradiance[2] > 3.0 * blue.irradiance[0]);
        assert!(rust.irradiance[0] > 3.0 * rust.irradiance[2]);
        // І зелений у них однаковий — тобто різниця саме в кольорі, а не в
        // загальній яскравості.
        assert!((blue.irradiance[1] - rust.irradiance[1]).abs() < 1e-12);
    }
}
