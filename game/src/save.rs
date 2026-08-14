//! Сейв: стан, план і крок інтегратора (ROADMAP J6, PROJECT.md §4).
//!
//! Правило 4 з §4 звучить так: **стан інтегратора входить у сейв.** Адаптивний
//! крок означає, що послідовність кроків залежить від історії; якщо після
//! завантаження почати зі «свіжого» кроку, траєкторія розійдеться з тією, що
//! була до збереження, а в N-body розбіжність росте експоненційно.
//!
//! H1 виміряв, скільки це коштує на одному прогоні: 7148 семплів замість 101 і
//! розбіжність 1.9 мм. J3 виміряв на маневрі: 1.4% зайвої роботи. Тут ціна
//! інша й найгірша — сейв, який дає **іншу гру**.
//!
//! ## Чого в сейві немає
//!
//! Траєкторії. §4: «сейв = стан + план маневрів + стан інтегратора (не вся
//! траєкторія)». Наслідок, який варто знати наперед: після завантаження
//! намальованої історії немає — вона відновлюється лише вперед, від точки
//! збереження. Це рішення дизайну, а не економія місця; якщо історія колись
//! знадобиться, вона піде окремим (і викидним) файлом.
//!
//! ## Чому шістнадцяткові біти, а не числа
//!
//! Сейв мусить відтворювати гру **бітово**, а десятковий друк — це домовленість
//! між форматувальником і парсером. У Rust вона надійна (найкоротший запис, що
//! читається назад точно), але C6 уже фіксував протилежний випадок: друк
//! `double` у десятковий текст — справа libc, і саме тому CSV не входять у
//! звірку детермінізму. Тут ціна помилки вища за читабельність, тож у файл іде
//! `to_bits`, а десяткове значення лишається поруч **коментарем** — для ока, і
//! парсер його не читає.

use std::fmt::Write as _;
use std::path::Path;

use core_rs::{State, Vec3d};

use crate::plan::{Frame, Manoeuvre, Plan};
use crate::world::World;

/// Версія формату. Зміниться — старі сейви мусять голосно не читатися, а не
/// тихо читатися не так.
const MAGIC: &str = "space_sim save v1";

pub struct SavedVessel {
    pub name: String,
    /// Стан, з якого продовжувати: остання **межа ланки не пізніша за
    /// курсор**, а не кінець порахованого.
    ///
    /// Не кінець — бо прогноз попереду курсора в сейв не входить, і
    /// відновлювати гру з нього означало б стрибнути на тижні вперед. Не сам
    /// курсор — бо з довільної точки продовжити бітово неможливо: крок
    /// інтегратора зберігається на межах ланок, і тільки там (`core/prop.h`).
    ///
    /// Отже зерно сейву — ланка. Скільки це часу, залежить від траєкторії, і
    /// зменшується разом із `world::LEG`.
    pub tip: State,
    /// Крок інтегратора. Без нього сейв дає іншу траєкторію.
    pub step: f64,
    pub horizon_end: f64,
    pub plan: Plan,
    /// Скільки маневрів плану вже вшито в `tip`.
    ///
    /// Зберігається явно, хоч і виводиться з часів: стан до й після імпульсу
    /// мають **однаковий час**, тож правило «застосувати все, що не пізніше»
    /// виконало б маневр удвічі, а «все, що раніше» — жодного разу, якби
    /// точка перезапуску колись стала пост-імпульсною. Число в файлі знімає
    /// це питання назавжди.
    pub applied: usize,

    /// Площа, маса й коефіцієнт відбиття (ROADMAP K6b).
    ///
    /// У сейві не для повноти опису, а тому що без них завантажений апарат
    /// летів би крізь іншу модель сил, ніж збережений, і траєкторія після
    /// завантаження розійшлася б з тією, що була до нього — рівно те, чого
    /// PROJECT.md §4 вимагає не допустити для кроку інтегратора, з тієї ж
    /// причини й того ж масштабу.
    pub params: Option<core_rs::VesselParams>,
}

pub struct Save {
    pub t: f64,
    pub warp: f64,
    pub vessels: Vec<SavedVessel>,
}

impl Save {
    /// Знімає сейв зі світу.
    ///
    /// Курсор зберігається як є, а стан кожного апарата — з останньої межі
    /// ланки не пізнішої за нього. Після завантаження горизонт наздоганяє
    /// курсор сам (годинник назад не ходить, `crate::clock`), і рахує він при
    /// цьому рівно ті самі ланки, що були.
    pub fn of(world: &World) -> Save {
        let cursor = world.clock().t();

        Save {
            t: cursor,
            warp: world.clock().warp(),
            vessels: world
                .vessels()
                .iter()
                .map(|v| {
                    let resume =
                        crate::leg::restart_at(v.trajectory.legs(), v.trajectory.start(), cursor);

                    SavedVessel {
                        name: v.name.clone(),
                        tip: resume.state,
                        step: resume.step,
                        horizon_end: v.horizon_end,
                        plan: v.plan.clone(),
                        // Точка перезапуску — це завжди стан ДО імпульсу
                        // (ланка закінчується перед ним), тож маневр рівно в
                        // цей момент іще не застосований.
                        applied: v
                            .plan
                            .manoeuvres()
                            .iter()
                            .take_while(|m| m.t < resume.state.t)
                            .count(),
                        params: v.params,
                    }
                })
                .collect(),
        }
    }

    /// Будує світ із сейву на вже завантаженій ефемериді.
    pub fn into_world(
        self,
        eph: std::sync::Arc<core_rs::Ephemeris>,
        cfg: core_rs::PropConfig,
    ) -> Result<World, core_rs::CoreError> {
        // Годинник ставиться на збережений курсор, хоч траєкторії там ще
        // немає: горизонт наздожене його першими ж тіками, а назад курсор не
        // ходить (`crate::clock`). Доти снапшот чесно каже `Stall::Horizon`.
        let mut world = World::with_ephemeris(eph, cfg, self.t, self.warp)?;

        for vessel in self.vessels {
            world.add_saved_vessel(vessel);
        }

        Ok(world)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_text()).map_err(|e| e.to_string())
    }

    pub fn read(path: &Path) -> Result<Save, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Save::from_text(&text)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{MAGIC}");
        let _ = writeln!(out, "t {} # {:e}", hex(self.t), self.t);
        let _ = writeln!(out, "warp {} # {:e}", hex(self.warp), self.warp);

        for vessel in &self.vessels {
            let _ = writeln!(out, "vessel {}", vessel.name);
            let _ = writeln!(out, "  tip {}", state_line(&vessel.tip));
            let _ = writeln!(out, "  step {} # {:e}", hex(vessel.step), vessel.step);
            let _ = writeln!(
                out,
                "  horizon_end {} # {:e}",
                hex(vessel.horizon_end),
                vessel.horizon_end
            );
            let _ = writeln!(out, "  applied {}", vessel.applied);
            if let Some(p) = vessel.params {
                let _ = writeln!(
                    out,
                    "  params {} {} {} {} # {:e} kg, {:e} m^2, cr {:e}, cd {:e}",
                    hex(p.mass_kg),
                    hex(p.area_m2),
                    hex(p.cr),
                    hex(p.cd),
                    p.mass_kg,
                    p.area_m2,
                    p.cr,
                    p.cd
                );
            }
            for m in vessel.plan.manoeuvres() {
                let frame = match m.frame {
                    Frame::Inertial => "inertial".to_string(),
                    Frame::Vnb { body } => format!("vnb:{body}"),
                };
                let _ = writeln!(
                    out,
                    "  manoeuvre {} {} {} {} {frame} # t={:e} dv=({:e}, {:e}, {:e})",
                    hex(m.t),
                    hex(m.dv[0]),
                    hex(m.dv[1]),
                    hex(m.dv[2]),
                    m.t,
                    m.dv[0],
                    m.dv[1],
                    m.dv[2]
                );
            }
        }

        out
    }

    pub fn from_text(text: &str) -> Result<Save, String> {
        let mut lines = text.lines();

        match lines.next().map(str::trim) {
            Some(MAGIC) => {}
            other => return Err(format!("це не сейв цього формату: {other:?}")),
        }

        let mut t = None;
        let mut warp = None;
        let mut vessels: Vec<SavedVessel> = Vec::new();

        for line in lines {
            // Коментар — усе після `#`. Саме там лежать десяткові значення
            // для ока, і парсер про них не знає нічого.
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let mut words = line.split_whitespace();
            let key = words.next().unwrap_or("");

            match key {
                "t" => t = Some(number(&mut words, "t")?),
                "warp" => warp = Some(number(&mut words, "warp")?),
                "vessel" => vessels.push(SavedVessel {
                    name: words.collect::<Vec<_>>().join(" "),
                    tip: State::default(),
                    step: 0.0,
                    horizon_end: 0.0,
                    plan: Plan::new(),
                    applied: 0,
                    params: None,
                }),
                _ => {
                    let vessel = vessels
                        .last_mut()
                        .ok_or_else(|| format!("'{key}' до першого 'vessel'"))?;

                    match key {
                        "tip" => {
                            let mut values = [0.0; 7];
                            for (index, slot) in values.iter_mut().enumerate() {
                                *slot = number(&mut words, &format!("tip[{index}]"))?;
                            }
                            vessel.tip = State {
                                r: Vec3d {
                                    x: values[0],
                                    y: values[1],
                                    z: values[2],
                                },
                                v: Vec3d {
                                    x: values[3],
                                    y: values[4],
                                    z: values[5],
                                },
                                t: values[6],
                            };
                        }
                        "step" => vessel.step = number(&mut words, "step")?,
                        "horizon_end" => vessel.horizon_end = number(&mut words, "horizon_end")?,
                        // Відсутній рядок — це `None`, безмасова пробна
                        // частинка: сейви, написані до K6b, читаються далі
                        // й означають рівно те, що означали.
                        "params" => {
                            let mass_kg = number(&mut words, "params[mass]")?;
                            let area_m2 = number(&mut words, "params[area]")?;
                            let cr = number(&mut words, "params[cr]")?;
                            // Відсутнє — нуль, тобто «цей апарат не відчуває
                            // повітря»: сейви, написані до K7b, читаються далі
                            // й означають рівно те, що означали. Той самий
                            // договір, що й для всього рядка `params` вище.
                            let cd = match words.clone().next() {
                                Some(w) if !w.starts_with('#') => number(&mut words, "params[cd]")?,
                                _ => 0.0,
                            };
                            vessel.params = Some(core_rs::VesselParams {
                                mass_kg,
                                area_m2,
                                cr,
                                cd,
                            });
                        }
                        "applied" => {
                            vessel.applied = words
                                .next()
                                .ok_or("applied без значення")?
                                .parse()
                                .map_err(|_| "applied не число".to_string())?;
                        }
                        "manoeuvre" => {
                            let t = number(&mut words, "manoeuvre.t")?;
                            let dv = [
                                number(&mut words, "manoeuvre.dv0")?,
                                number(&mut words, "manoeuvre.dv1")?,
                                number(&mut words, "manoeuvre.dv2")?,
                            ];
                            let frame = words.next().ok_or("маневр без фрейму")?;
                            let frame = match frame {
                                "inertial" => Frame::Inertial,
                                other => match other.strip_prefix("vnb:") {
                                    Some(body) => Frame::Vnb {
                                        body: body
                                            .parse()
                                            .map_err(|_| format!("фрейм '{other}'"))?,
                                    },
                                    None => return Err(format!("невідомий фрейм '{other}'")),
                                },
                            };
                            vessel.plan.insert(Manoeuvre { t, dv, frame });
                        }
                        other => return Err(format!("невідомий ключ '{other}'")),
                    }
                }
            }
        }

        Ok(Save {
            t: t.ok_or("у сейві немає 't'")?,
            warp: warp.ok_or("у сейві немає 'warp'")?,
            vessels,
        })
    }
}

/// Число як біти. Коментар для ока дописує той, хто пише рядок, — **один**
/// на рядок і в кінці: `#` з'їдає все, що після нього.
fn hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn state_line(state: &State) -> String {
    format!(
        "{:016x} {:016x} {:016x} {:016x} {:016x} {:016x} {:016x} \
         # r=({:e}, {:e}, {:e}) t={:e}",
        state.r.x.to_bits(),
        state.r.y.to_bits(),
        state.r.z.to_bits(),
        state.v.x.to_bits(),
        state.v.y.to_bits(),
        state.v.z.to_bits(),
        state.t.to_bits(),
        state.r.x,
        state.r.y,
        state.r.z,
        state.t
    )
}

fn number<'a>(words: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<f64, String> {
    let word = words.next().ok_or_else(|| format!("{what} без значення"))?;
    let raw = u64::from_str_radix(word, 16).map_err(|_| format!("{what}: '{word}' не біти"))?;
    Ok(f64::from_bits(raw))
}

/// Куди пише гра за замовчуванням.
pub fn default_path() -> std::path::PathBuf {
    std::path::PathBuf::from("build/save.txt")
}

/// Зручність для того, хто зберігає світ у нитці симуляції.
pub fn write_world(world: &World, path: &Path) -> Result<(), String> {
    Save::of(world).write(path)
}
