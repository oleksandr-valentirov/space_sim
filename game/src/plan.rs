//! План маневрів (ROADMAP J3, PROJECT.md §8).
//!
//! Маневр — це `(час, Δv у фреймі)`, план — список маневрів, перерахунок
//! каскадний. Тобто рівно те, що §8 називає флайт-планером, у мінімальній
//! формі, яку вже можна виконати: **імпульсний** Δv.
//!
//! ## Чому імпульсний, а не з тривалістю горіння
//!
//! Скінченне горіння потребує тяги в силовій моделі C, а її там немає й не
//! буде до M3.5. Імпульс же виконується тим, що вже є: пропагувати до моменту
//! запалення, додати Δv до швидкості, продовжити. Форма плану до тривалості
//! готова — з'явиться поле, а не інший механізм.
//!
//! ## Межа детермінізму проходить тут
//!
//! PROJECT.md §4: «Симуляція заданого плану мусить збігатися біт-у-біт; те, як
//! гравець цей план придумав, — ні». План — це **дані**: два числа й фрейм.
//! Lambert, porkchop і диференціальна корекція можуть давати трохи різні числа
//! на різних машинах, і це дозволено; те, що з отриманого плану вийде, —
//! ні.
//!
//! Перетворення Δv з фрейму в інерціальні координати робиться **всередині**
//! межі детермінізму: там лише `+ − * /` і `sqrt`, тобто ті самі операції, які
//! CLAUDE.md дозволяє в циклі інтегрування (інваріант 3). Тригонометрії тут
//! немає й бути не має.

use core_rs::State;

/// У чому задані компоненти Δv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Барицентричний інерціальний — той самий, у якому рахується все інше.
    /// Так задають Δv солвери: Lambert віддає вектори швидкості, не «стільки
    /// вперед».
    Inertial,

    /// Уздовж швидкості / нормаль до площини / назовні, відносно тіла `body`.
    ///
    /// Так думає гравець: «сто метрів за секунду вперед». Обов'язково
    /// відносно тіла: у барицентричних координатах швидкість апарата біля
    /// Землі — це переважно швидкість самої Землі навколо Сонця, і «вперед»
    /// означало б уздовж земної орбіти.
    Vnb { body: i32 },
}

/// Один маневр.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Manoeuvre {
    /// Момент запалення, секунди від епохи ассета. Абсолютний.
    ///
    /// Не «на третьому перицентрі»: якорі на події теж будуть, але вони
    /// розв'язуються в абсолютний час при перерахунку й у такому вигляді
    /// лягають у сейв. Інакше сейв означав би різне залежно від того, коли
    /// його прочитали.
    pub t: f64,
    /// Компоненти у [`Frame`], м/с.
    pub dv: [f64; 3],
    pub frame: Frame,
}

impl Manoeuvre {
    /// Δv у барицентричних інерціальних координатах.
    ///
    /// `body` — стан тіла відліку в момент маневру; для [`Frame::Inertial`]
    /// не потрібен і не читається.
    pub fn dv_inertial(&self, vessel: &State, body: Option<&State>) -> [f64; 3] {
        match self.frame {
            Frame::Inertial => self.dv,
            Frame::Vnb { .. } => {
                let Some(body) = body else {
                    // Викликач зобов'язаний дати тіло; без нього базису немає.
                    // Мовчки взяти інерціальний означало б виконати не той
                    // маневр і не сказати про це.
                    return [0.0, 0.0, 0.0];
                };

                let r = [
                    vessel.r.x - body.r.x,
                    vessel.r.y - body.r.y,
                    vessel.r.z - body.r.z,
                ];
                let v = [
                    vessel.v.x - body.v.x,
                    vessel.v.y - body.v.y,
                    vessel.v.z - body.v.z,
                ];

                let prograde = normalize(v);
                let normal = normalize(cross(r, v));
                // Довершує праву трійку: назовні від тіла в площині орбіти.
                let outward = cross(prograde, normal);

                [
                    self.dv[0] * prograde[0] + self.dv[1] * normal[0] + self.dv[2] * outward[0],
                    self.dv[0] * prograde[1] + self.dv[1] * normal[1] + self.dv[2] * outward[1],
                    self.dv[0] * prograde[2] + self.dv[1] * normal[2] + self.dv[2] * outward[2],
                ]
            }
        }
    }

    /// Тіло, відносно якого заданий фрейм, якщо таке є.
    pub fn frame_body(&self) -> Option<i32> {
        match self.frame {
            Frame::Inertial => None,
            Frame::Vnb { body } => Some(body),
        }
    }
}

/// Список маневрів, упорядкований за часом.
///
/// Порядок — інваріант типу, а не домовленість: сегментний цикл бере
/// наступний маневр за індексом і зупиняється на його часі, тож
/// невпорядкований план означав би прогін у минуле.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    manoeuvres: Vec<Manoeuvre>,
}

impl Plan {
    pub fn new() -> Plan {
        Plan::default()
    }

    pub fn manoeuvres(&self) -> &[Manoeuvre] {
        &self.manoeuvres
    }

    pub fn is_empty(&self) -> bool {
        self.manoeuvres.is_empty()
    }

    pub fn len(&self) -> usize {
        self.manoeuvres.len()
    }

    pub fn get(&self, index: usize) -> Option<&Manoeuvre> {
        self.manoeuvres.get(index)
    }

    /// Додає маневр, зберігаючи порядок за часом.
    pub fn insert(&mut self, m: Manoeuvre) {
        let at = self.manoeuvres.partition_point(|other| other.t <= m.t);
        self.manoeuvres.insert(at, m);
    }

    /// Найраніший момент, у якому два плани розходяться.
    ///
    /// Це і є точка, з якої треба перерахувати, і ніде більше вона не
    /// береться: порівнювати траєкторії було б і дорожче, і пізніше — вони
    /// розходяться вже після маневру, а не в ньому.
    pub fn diverges_from(&self, other: &Plan) -> Option<f64> {
        let mine = &self.manoeuvres;
        let theirs = &other.manoeuvres;

        for (a, b) in mine.iter().zip(theirs.iter()) {
            if a != b {
                // Раніший із двох: маневр міг і зникнути, і з'явитися раніше.
                return Some(a.t.min(b.t));
            }
        }

        // Однакові настільки, наскільки перекриваються; лишок — це поява або
        // зникнення хвоста.
        match mine.len().cmp(&theirs.len()) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Less => Some(theirs[mine.len()].t),
            std::cmp::Ordering::Greater => Some(mine[theirs.len()].t),
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_rs::Vec3d;

    fn state(r: [f64; 3], v: [f64; 3]) -> State {
        State {
            r: Vec3d {
                x: r[0],
                y: r[1],
                z: r[2],
            },
            v: Vec3d {
                x: v[0],
                y: v[1],
                z: v[2],
            },
            t: 0.0,
        }
    }

    /// Базис VNB ортонормований і орієнтований так, як обіцяно.
    ///
    /// Кругова орбіта в площині xy: «вперед» мусить лягти на +y, «нормаль» на
    /// +z, «назовні» на +x. Помилка в порядку векторного добутку дала б
    /// дзеркальний базис, і маневр «вперед» гальмував би.
    #[test]
    fn the_vnb_basis_points_where_it_says() {
        let vessel = state([1.0e7, 0.0, 0.0], [0.0, 3.0e3, 0.0]);
        let body = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);

        let along = Manoeuvre {
            t: 0.0,
            dv: [10.0, 0.0, 0.0],
            frame: Frame::Vnb { body: 3 },
        };
        assert_eq!(along.dv_inertial(&vessel, Some(&body)), [0.0, 10.0, 0.0]);

        let normal = Manoeuvre {
            dv: [0.0, 10.0, 0.0],
            ..along
        };
        assert_eq!(normal.dv_inertial(&vessel, Some(&body)), [0.0, 0.0, 10.0]);

        let outward = Manoeuvre {
            dv: [0.0, 0.0, 10.0],
            ..along
        };
        assert_eq!(outward.dv_inertial(&vessel, Some(&body)), [10.0, 0.0, 0.0]);
    }

    /// Фрейм рахується відносно тіла, а не барицентра.
    ///
    /// Тіло, що само летить швидше за апарат, — це саме той випадок, у якому
    /// різниця не косметична: у барицентричних координатах «вперед» показало б
    /// уздовж руху тіла.
    #[test]
    fn the_frame_follows_the_body_not_the_barycentre() {
        let body = state([0.0, 0.0, 0.0], [3.0e4, 0.0, 0.0]);
        let vessel = state([1.0e7, 0.0, 0.0], [3.0e4, 3.0e3, 0.0]);

        let along = Manoeuvre {
            t: 0.0,
            dv: [10.0, 0.0, 0.0],
            frame: Frame::Vnb { body: 3 },
        };
        assert_eq!(along.dv_inertial(&vessel, Some(&body)), [0.0, 10.0, 0.0]);
    }

    /// Розбіжність планів знаходиться в найранішій зміні, з обох боків.
    #[test]
    fn divergence_finds_the_earliest_change() {
        let m = |t: f64, dv: f64| Manoeuvre {
            t,
            dv: [dv, 0.0, 0.0],
            frame: Frame::Inertial,
        };

        let mut a = Plan::new();
        a.insert(m(100.0, 1.0));
        a.insert(m(200.0, 2.0));

        assert_eq!(a.diverges_from(&a.clone()), None);

        // Змінений другий маневр.
        let mut b = a.clone();
        b.manoeuvres[1] = m(200.0, 5.0);
        assert_eq!(a.diverges_from(&b), Some(200.0));

        // Зсунутий у часі — раніший із двох моментів, бо перерахувати треба
        // від того, у якому плани вже різні.
        let mut c = a.clone();
        c.manoeuvres[1] = m(150.0, 2.0);
        assert_eq!(a.diverges_from(&c), Some(150.0));

        // Дописаний хвіст.
        let mut d = a.clone();
        d.insert(m(300.0, 3.0));
        assert_eq!(a.diverges_from(&d), Some(300.0));
        assert_eq!(d.diverges_from(&a), Some(300.0));
    }

    /// Вставка тримає порядок за часом, як би її не кликали.
    #[test]
    fn insertion_keeps_the_order() {
        let m = |t: f64| Manoeuvre {
            t,
            dv: [0.0; 3],
            frame: Frame::Inertial,
        };

        let mut plan = Plan::new();
        for t in [300.0, 100.0, 200.0, 50.0] {
            plan.insert(m(t));
        }

        let times: Vec<f64> = plan.manoeuvres().iter().map(|m| m.t).collect();
        assert_eq!(times, vec![50.0, 100.0, 200.0, 300.0]);
    }
}
