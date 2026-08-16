//! Тонмапер: стиснення яскравостей понад одиницю (ROADMAP, T5c3).
//!
//! Двійник `shaders/tonemap.slang`. Причини кожного вибору — у шапці того
//! файлу; тут те, що потрібно викликачам і перевіркам.
//!
//! Головна властивість, на яку спираються оракули всього етапу T: **нижче
//! коліна крива є тотожністю**, і не «майже», а бітово. Уся дифузна робота
//! живе там — відбивна здатність Місяця 0.02…0.25, мозаїка, правило матеріалу
//! — тож прохід не зрушив жодного з уже виміряних чисел.

/// Де крива перестає бути тотожністю.
///
/// 0.8, а не 1.0: коліно рівно в одиниці дало б злам похідної там, де око
/// найчутливіше — на межі відблиску. Восьма десята лишає під стиснення
/// п'яту частину шкали й не чіпає нічого з того, що етап T виміряв.
pub const KNEE: f64 = 0.8;

/// Стиснути один канал.
pub fn compress(value: f64) -> f64 {
    if value <= KNEE {
        return value;
    }
    let d = 1.0 - KNEE;
    1.0 - d * d / (value - 2.0 * KNEE + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Нижче коліна крива нічого не робить — бітово.
    ///
    /// На цьому стоїть кожен оракул етапу T, включно зі звіркою «байт проти
    /// виміряної відбивної здатності» (T5b). Крива, яка чіпала б ці значення,
    /// зробила б ту звірку неможливою.
    #[test]
    fn below_the_knee_nothing_moves() {
        for k in 0..=800 {
            let value = f64::from(k) / 1000.0;
            assert_eq!(
                compress(value).to_bits(),
                value.to_bits(),
                "{value} зрушило нижче коліна"
            );
        }
    }

    /// Понад коліном крива монотонна й ніколи не досягає одиниці.
    #[test]
    fn above_the_knee_it_climbs_towards_one_and_never_reaches_it() {
        let mut previous = KNEE;
        for k in 0..2000 {
            let value = KNEE + f64::from(k) * 0.01;
            let got = compress(value);
            assert!(got >= previous, "{value} дало {got} після {previous}");
            assert!(got < 1.0, "{value} дало {got} — це вже одиниця");
            previous = got;
        }
        // Дуже яскраве все ж мусить дійти близько до одиниці, інакше відблиск
        // буде сірим замість білого.
        assert!(compress(1.0e4) > 0.999);
    }

    /// Крива гладка в коліні: значення й нахил збігаються з обох боків.
    ///
    /// Злам похідної видно оком як кільце навколо відблиску — саме той
    /// артефакт, проти якого прохід і робиться.
    #[test]
    fn the_knee_has_no_corner_in_it() {
        let step = 1e-6;
        let below = (compress(KNEE) - compress(KNEE - step)) / step;
        let above = (compress(KNEE + step) - compress(KNEE)) / step;
        println!("  нахил до коліна {below:.6}, після {above:.6}");
        assert!((below - 1.0).abs() < 1e-4);
        assert!((above - 1.0).abs() < 1e-4);
    }

    /// Відблиск, який раніше зрізався, тепер лишається різним.
    ///
    /// Дві яскравості, обидві понад одиницю, мусять дати **різні** байти —
    /// інакше прохід не робить того, заради чого існує. Числа взяті з
    /// виміряного: `roughness = 0.35` дає в піку близько 3.7.
    #[test]
    fn two_highlights_that_used_to_clip_are_still_different() {
        let a = crate::srgb::linear_to_byte(compress(2.0));
        let b = crate::srgb::linear_to_byte(compress(3.7));
        let c = crate::srgb::linear_to_byte(compress(12.0));
        println!("  2.0 → {a}, 3.7 → {b}, 12.0 → {c}");
        assert!(a < b && b < c, "відблиски злиплися: {a}, {b}, {c}");
        // А без стиснення всі троє були б рівно 255.
        assert_eq!(crate::srgb::linear_to_byte(2.0), 255);
        assert_eq!(crate::srgb::linear_to_byte(3.7), 255);
    }
}
