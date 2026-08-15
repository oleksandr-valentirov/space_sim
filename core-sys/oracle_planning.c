/* Оракул для FFI-декларацій планування (ROADMAP L3, борг D1).
 *
 * Другий оракул, а не ще кілька тегів у першому, і причина рівно одна:
 * ЦЕЙ ЛІНКУЄТЬСЯ З `-lm`. `core-sys/oracle.c` лінкується без неї навмисно —
 * лінкування там є перевіркою того, що в рантаймову зону не просочилася
 * тригонометрія. Дописати сюди `lambert_solve`, який кличе `acos`, `sinh` і
 * `cosh`, означало б зняти ту перевірку заради зручності.
 *
 * Те саме твердження на рівні бібліотек: `libcore_planning.a` окремо від
 * `libcore.a` (core-sys/build.rs), бо межа детермінізму проходить по
 * пропагації, а не по плануванню (PROJECT.md §4).
 *
 * Що саме перевіряється звіркою. `lambert_solve` — перша функція межі, яка
 * приймає структуру **за значенням**, а не вказівником. Vec3d з трьох double
 * не влазить у регістри жодного з наших ABI, тож вона їде через пам'ять, і
 * якби Rust і C розійшлися в цьому, результат був би не падінням, а
 * правдоподібними швидкостями. Тому звірка бітова.
 *
 * Формат той самий, що в oracle.c: перше поле — тег, далі числа в %.17g.
 *
 *   lam  <v1x> <v1y> <v1z> <v2x> <v2y> <v2z>   успішний розв'язок
 *   lerr <code>                                 код відмови
 *   pork <k> <t1> <tof> <v_inf_depart> <v_inf_arrive>   клітинка сітки
 *
 * `pork` з'явився з U5a: `porkchop_compute_eph` читає ефемериду, тож оракул
 * тепер таки читає ассет — і запуск з кореня репозиторію став обов'язковим,
 * а не косметичним. */

#include "ephemeris.h"
#include "lambert.h"
#include "porkchop.h"

#include <stdio.h>

/* Геліоцентричний переліт, бо саме для нього Lambert і існує в цій грі
 * (PROJECT.md §8, porkchop). mu Сонця з data/horizons; радіуси й час
 * перельоту — круглі числа порядку земної та марсіанської орбіт, не
 * ефемеридні: оракул перевіряє межу, а не астрономію, і прив'язка до ассета
 * зробила б його чутливим до `make cook`.
 *
 * Площина не збігається з xy: третя компонента ненульова в обох точках. Це
 * той самий урок, що K7b виніс із градієнта опору — перевірка, поставлена
 * там, де компонента тотожно нульова, мовчить про цілий стовпець. */
#define MU_SUN 1.32712440018e20

static const Vec3d R1 = { 1.4959787e11, 0.0, 0.0 };
static const Vec3d R2 = { -1.9e11, 1.1e11, 8.0e9 };

#define TOF_S (2.5e7) /* ~289 діб, порядок реального вікна до Марса */

#define ASSET "data/fixture/earth_moon.eph"
#define DAY 86400.0

/* Сітка Земля → Місяць: три дати відходу, два часи перельоту. Дрібна
 * навмисно — оракул перевіряє розкладку й порядок полів, а не астрономію. */
static void porkchop(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "oracle_planning: не читається %s\n", ASSET);
        fprintf(stderr, "  запускати з кореня репозиторію\n");
        return;
    }

    const int EARTH = 3, MOON = 4;
    double t1s[3] = { 0.0, 3.0 * DAY, 6.0 * DAY };
    double tofs[2] = { 4.0 * DAY, 5.0 * DAY };

    PorkchopPoint grid[6];
    size_t n = 0;
    if (porkchop_compute_eph(eph, EARTH, MOON, eph_body_mu(eph, EARTH), 1,
                             t1s, 3, tofs, 2, grid, 6, &n) != CORE_OK) {
        fprintf(stderr, "oracle_planning: сітка не порахувалась\n");
        eph_free(eph);
        return;
    }

    for (size_t k = 0; k < n; k++) {
        printf("pork %zu %.17g %.17g %.17g %.17g\n",
               k, grid[k].t1, grid[k].tof,
               grid[k].v_inf_depart, grid[k].v_inf_arrive);
    }

    eph_free(eph);
}

static void print_pair(const char *tag, Vec3d v1, Vec3d v2)
{
    printf("%s %.17g %.17g %.17g %.17g %.17g %.17g\n",
           tag, v1.x, v1.y, v1.z, v2.x, v2.y, v2.z);
}

int main(void)
{
    Vec3d v1, v2;

    /* Пряма гілка й зворотна. Обидві, бо `prograde` — це знак z-компоненти
     * моменту імпульсу, а не «коротка чи довга дуга» (ROADMAP, «Фізика й
     * пропагація»), і переплутаний int тут дав би цілком правдоподібний
     * розв'язок іншої задачі. */
    if (lambert_solve(R1, R2, TOF_S, MU_SUN, 1, 0, &v1, &v2) != CORE_OK) {
        fprintf(stderr, "oracle_planning: прямий переліт не зійшовся\n");
        return 1;
    }
    print_pair("lam", v1, v2);

    if (lambert_solve(R1, R2, TOF_S, MU_SUN, 0, 0, &v1, &v2) != CORE_OK) {
        fprintf(stderr, "oracle_planning: зворотний переліт не зійшовся\n");
        return 1;
    }
    print_pair("lam", v1, v2);

    /* І відмова. Код повернення теж перетинає межу, і `CoreResult` як `c_int`
     * з константами (а не Rust-енум) має сенс лише тоді, коли хтось справді
     * звіряє значення. n_revs != 0 — задокументована відмова lambert.h. */
    printf("lerr %d\n", (int)lambert_solve(R1, R2, TOF_S, MU_SUN, 1, 1, &v1, &v2));

    /* Друга відмова, іншого походження: вироджена геометрія. r1 і r2 на одній
     * прямій через початок — площина перельоту невизначена. */
    Vec3d opposite = { -R1.x, -R1.y, -R1.z };
    printf("lerr %d\n",
           (int)lambert_solve(R1, opposite, TOF_S, MU_SUN, 1, 0, &v1, &v2));

    porkchop();

    return 0;
}
