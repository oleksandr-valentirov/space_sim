//! Орбітальна камера: чим гравець рухає погляд (ROADMAP I2).
//!
//! Стан — три числа в `double`: два кути й висота над поверхнею. Позиція
//! камери з них **виводиться**, а не накопичується, і це головне рішення
//! модуля. Камера, яка інтегрує зсуви у власну позицію, з часом сповзає з
//! сфери, і жодного моменту, коли це стало помітно, не існує.
//!
//! Тут немає ні GPU, ні вікна, ні `winit`: на вході числа, на виході
//! [`Camera`]. Тому все, що модуль обіцяє, перевіряється без адаптера —
//! а події вікна лишаються тонким шаром у `app`, який лише перекладає їх
//! у ці виклики.

use crate::camera::Camera;
use crate::sphere;

/// Найнижча висота, метри. Стільки ж, скільки найближча точка прольоту F5:
/// нижче меш 64×128 — це вже не сфера, а окрема грань під носом камери, і
/// міряти на ній нічого.
pub const MIN_ALTITUDE_M: f64 = 10.0;

/// Найвища. 10¹¹ м ≈ 0.7 а.о. — на цій відстані F4 виміряв, що
/// camera-relative тримається до останньої цифри; далі вже нічого не
/// перевірено, тож туди камеру не пускаємо.
pub const MAX_ALTITUDE_M: f64 = 1.0e11;

/// Скільки радіан на піксель тягне миша. Півекрана (≈600 px) на пів оберту —
/// звичний для орбітальних камер темп.
const RADIANS_PER_PIXEL: f64 = std::f64::consts::PI / 600.0;

/// У скільки разів один «клац» колеса міняє висоту.
///
/// Геометрично, не додаванням: від 10 м до 10¹¹ м десять порядків, і будь-який
/// сталий крок у метрах або нерухомий на одному кінці, або непридатний на
/// іншому.
const ZOOM_PER_NOTCH: f64 = 1.25;

/// Куди не можна доводити нахил.
///
/// Рівно на полюсі напрямок погляду збігається з орієнтиром «вгору», їхній
/// векторний добуток — нуль, і базис камери перетворюється на NaN. Це не
/// теоретичний ризик: користувач доводить камеру до полюса за секунду.
const PITCH_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 1.0e-3;

pub struct Orbit {
    /// Азимут навколо осі z.
    yaw: f64,
    /// Підйом над площиною xy, обмежений [`PITCH_LIMIT`].
    pitch: f64,
    /// Висота над поверхнею, метри.
    altitude: f64,
}

impl Default for Orbit {
    /// Той самий погляд, що [`crate::frame::default_camera`] — тобто той, у
    /// якому виміряне покриття кадру звіряється з аналітичною формулою.
    /// Інтерактивний кадр починається рівно там, де його перевіряють.
    fn default() -> Self {
        Orbit {
            yaw: 0.0,
            pitch: 0.0,
            altitude: crate::frame::DEFAULT_ALTITUDE_M,
        }
    }
}

impl Orbit {
    pub fn altitude(&self) -> f64 {
        self.altitude
    }

    /// Відстань від центра планети.
    pub fn distance(&self) -> f64 {
        sphere::EARTH_RADIUS_M + self.altitude
    }

    /// Тягнення миші на `dx`, `dy` пікселів.
    pub fn drag(&mut self, dx: f64, dy: f64) {
        self.yaw += dx * RADIANS_PER_PIXEL;
        self.pitch = (self.pitch + dy * RADIANS_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// `notches` клацань колеса: додатні наближають.
    pub fn zoom(&mut self, notches: f64) {
        let factor = ZOOM_PER_NOTCH.powf(-notches);
        self.altitude = (self.altitude * factor).clamp(MIN_ALTITUDE_M, MAX_ALTITUDE_M);
    }

    pub fn camera(&self) -> Camera {
        let distance = self.distance();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();

        let position = [
            distance * cos_pitch * cos_yaw,
            distance * cos_pitch * sin_yaw,
            distance * sin_pitch,
        ];

        Camera::look_at(position, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Планета лишається точно попереду, під якими завгодно кутами.
    ///
    /// Це перевірка всього ланцюжка кути → позиція → базис камери за один
    /// раз: центр світу мусить лягти на вісь погляду, а його відстань —
    /// збігтися з `distance()`. Помилка в знаку, порядку множників чи
    /// переплутані sin/cos зсунули б центр убік.
    #[test]
    fn the_planet_stays_dead_ahead() {
        for yaw_steps in 0..8 {
            for pitch_steps in -3..=3 {
                let mut orbit = Orbit::default();
                orbit.drag(f64::from(yaw_steps) * 100.0, f64::from(pitch_steps) * 100.0);

                let centre = orbit.camera().relative([0.0, 0.0, 0.0]);
                let distance = orbit.distance();

                assert!(
                    centre[0].abs() < 1.0 && centre[1].abs() < 1.0,
                    "центр планети зсунувся вбік: {centre:?}"
                );
                assert!(
                    ((-f64::from(centre[2])) - distance).abs() / distance < 1e-6,
                    "відстань до центра {} проти очікуваної {distance}",
                    -f64::from(centre[2])
                );
            }
        }
    }

    /// Через полюс камера не перекидається й не стає NaN.
    ///
    /// Найдешевший спосіб отримати NaN у рушії — довести погляд рівно вздовж
    /// орієнтира «вгору». Користувач робить це за секунду тягнення.
    #[test]
    fn dragging_past_the_pole_stays_finite() {
        let mut orbit = Orbit::default();
        for _ in 0..100 {
            orbit.drag(0.0, 1000.0);
        }

        let p = orbit.camera().relative([0.0, 0.0, 0.0]);
        assert!(p.iter().all(|v| v.is_finite()), "камера дала NaN: {p:?}");
        assert!(orbit.pitch < std::f64::consts::FRAC_PI_2);

        for _ in 0..200 {
            orbit.drag(0.0, -1000.0);
        }
        let p = orbit.camera().relative([0.0, 0.0, 0.0]);
        assert!(p.iter().all(|v| v.is_finite()), "камера дала NaN: {p:?}");
        assert!(orbit.pitch > -std::f64::consts::FRAC_PI_2);
    }

    /// Наблизитися до планети ближче за поверхню не можна.
    #[test]
    fn zooming_in_forever_stops_at_the_surface() {
        let mut orbit = Orbit::default();
        for _ in 0..1000 {
            orbit.zoom(1.0);
        }
        assert_eq!(orbit.altitude(), MIN_ALTITUDE_M);

        for _ in 0..1000 {
            orbit.zoom(-1.0);
        }
        assert_eq!(orbit.altitude(), MAX_ALTITUDE_M);
    }

    /// Наближення й віддалення на ту саму кількість клацань повертають туди,
    /// звідки почали.
    ///
    /// Це і є твердження «масштаб геометричний»: у додаванні метрів воно було
    /// б так само правдивим, але висота 10 м після кроку в 10⁶ м стала б
    /// від'ємною ще на першому клацанні.
    #[test]
    fn zoom_is_geometric_and_reversible() {
        let mut orbit = Orbit::default();
        let start = orbit.altitude();

        for _ in 0..20 {
            orbit.zoom(1.0);
        }
        let closer = orbit.altitude();
        assert!(closer < start / 10.0, "двадцять клацань мали наблизити");

        for _ in 0..20 {
            orbit.zoom(-1.0);
        }
        assert!(
            (orbit.altitude() - start).abs() / start < 1e-9,
            "повернулися на {} замість {start}",
            orbit.altitude()
        );
    }

    /// Камера за замовчуванням — та сама, на якій міряють кадр.
    #[test]
    fn the_default_view_is_the_one_the_shot_test_measures() {
        let from_orbit = Orbit::default().camera();
        let from_frame = crate::frame::default_camera();

        assert_eq!(from_orbit.position(), from_frame.position());
    }
}
