/* Оракул для перевірки FFI-декларацій (ROADMAP D2).
 *
 * Друкує те, що `eph_body_state` повертає в C, щоб `tests/ffi.rs` могло
 * звірити з тим, що та сама функція повертає через Rust. Порівняння бітове.
 *
 * Навіщо окрема програма замість чисел, вписаних у тест. Помилка в межі не
 * падає — вона дає правдоподібні числа. Переплутані поля `State`, `int`
 * замість `size_t`, забутий `const` у сигнатурі: усе це компілюється й
 * повертає щось. Впаяні в тест літерали таку помилку зловили б, але
 * протухли б при першому `make cook` і не мали б способу оновитися — а тут
 * оракул перезбирається разом із ассетом, і звірка лишається живою.
 *
 * Це не частина ядра й не сценарій детермінізму: у `core/scenario/` він
 * змінив би `golden.txt`, а тут він просто риштування крейта.
 *
 * Друк у %.17g: сімнадцять значущих цифр однозначно відновлюють double, тож
 * текст посередині нічого не втрачає.
 *
 * Формат: перше поле — тег, далі числа в %.17g.
 *
 *   eph  <body> <t> <x> <y> <z> <vx> <vy> <vz>
 *   samp <k> <t> <x> <y> <z> <vx> <vy> <vz>      семпл прогону
 *   run  <count> <stop> <event> <step>           підсумок прогону
 *   end  <t> <x> <y> <z> <vx> <vy> <vz>          кінцевий стан прогону
 *
 * Прогонів два: без подій до заданого часу і з озброєним перицентром.
 * Другий важливий окремо — він проходить через `CoreEvent`, а структура з
 * enum, int і double поспіль це саме те місце, де розкладка й вирівнювання
 * розходяться тихо.
 *
 * Апарат заданий літералами, а не порахований: `sqrt` тут немає, бо оракул
 * лінкується без `libm` — так само, як сценарії детермінізму (build.rs).
 *
 * Запускається з кореня репозиторію. */

#include "ephemeris.h"
#include "prop.h"

#include <stdio.h>

#define ASSET "data/fixture/earth_moon.eph"
#define DAY 86400.0

/* Індекси в порядку кукера (core/cook/cook_fixture.c) і моменти всередині
 * 120-денного проміжку фікстури. Сонце й Місяць навмисно: перше майже не
 * рухається на цьому масштабі, друге рухається найшвидше, тож помилка в
 * розкладці полів на одному з них видна напевно. */
static const int BODIES[] = { 0, 3, 4 };
#define N_BODIES (sizeof BODIES / sizeof BODIES[0])

static const double TIMES[] = { 0.0, 30.0 * DAY, 119.0 * DAY };
#define N_TIMES (sizeof TIMES / sizeof TIMES[0])

/* Апарат на витягнутій навколоземній орбіті: зсув від Землі й швидкість,
 * задані числами. 0.8 колової швидкості на геостаціонарному радіусі — тобто
 * орбіта з перицентром, який є що шукати. */
#define VESSEL_T0 (1.0 * DAY)
#define VESSEL_DX 42164.0e3
#define VESSEL_VY 1967.84
#define VESSEL_VZ 1475.88

#define CAP 64

static void print_state(const char *tag, const State *s)
{
    printf("%s %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
           tag, s->t, s->r.x, s->r.y, s->r.z, s->v.x, s->v.y, s->v.z);
}

static int propagate(const EphemerisCtx *eph)
{
    State earth;
    if (eph_body_state(eph, 3, VESSEL_T0, &earth) != CORE_OK) {
        return 0;
    }

    State vessel;
    vessel.r = vec3(earth.r.x + VESSEL_DX, earth.r.y, earth.r.z);
    vessel.v = vec3(earth.v.x, earth.v.y + VESSEL_VY, earth.v.z + VESSEL_VZ);
    vessel.t = VESSEL_T0;

    PropConfig cfg;
    cfg.integrator = CORE_INTEG_DOP853;
    cfg.tol_m = 1e-2;
    cfg.h_max_s = 1800.0;
    cfg.max_steps = 0;

    PropagatorCtx *p = NULL;
    if (prop_create(eph, &cfg, &p) != CORE_OK) {
        return 0;
    }

    State samples[CAP];
    size_t n = 0;
    State final_state;
    CoreStopReason stop;
    int event = -1;
    double step = 0.0;

    if (prop_run(p, &vessel, NULL, VESSEL_T0 + 0.5 * DAY, NULL, 0, samples, CAP, &n,
                 &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    for (size_t k = 0; k < n; k++) {
        printf("samp %zu %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
               k, samples[k].t, samples[k].r.x, samples[k].r.y, samples[k].r.z,
               samples[k].v.x, samples[k].v.y, samples[k].v.z);
    }
    printf("run %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("end", &final_state);

    /* Той самий апарат, але прогін зупиняє подія. */
    CoreEvent ev;
    ev.kind = CORE_EVENT_PERIAPSIS;
    ev.body_id = 3;
    ev.param = 0.0;

    step = 0.0;
    if (prop_run(p, &vessel, NULL, VESSEL_T0 + 4.0 * DAY, &ev, 1, NULL, 0, &n,
                 &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("run %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("end", &final_state);

    /* Та сама ланка, але з матрицею переходу (ROADMAP K8). Друкується і
     * кінцевий стан, і крок: обіцянка межі в тому, що це бітово те саме,
     * що дав би prop_run, тож звірка мусить бачити обидва. */
    step = 0.0;
    double phi[36];
    if (prop_run_stm(p, &vessel, NULL, VESSEL_T0 + 0.5 * DAY, &final_state, phi,
                     &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("stmrun %.17g\n", step);
    print_state("stmend", &final_state);
    for (int i = 0; i < 36; i++) {
        printf("stm %d %.17g\n", i, phi[i]);
    }

    /* І та сама ланка з апаратом, який відчуває тиск світла (ROADMAP K6b).
     * Кожен аргумент межі має бути тут хоч раз ненульовим: `vessel` як
     * NULL уже надруковано вище, а вказівник, який ніхто не розіменовує,
     * не довів би, що поля структури оголошені в тому самому порядку. */
    VesselParams sail;
    sail.mass_kg = 1000.0;
    sail.area_m2 = 20.0;
    sail.cr = 1.3;

    step = 0.0;
    if (prop_run(p, &vessel, &sail, VESSEL_T0 + 0.5 * DAY, NULL, 0, samples,
                 CAP, &n, &final_state, &stop, &event, &step) != CORE_OK) {
        prop_free(p);
        return 0;
    }

    printf("srprun %zu %d %d %.17g\n", n, (int)stop, event, step);
    print_state("srpend", &final_state);

    prop_free(p);
    return 1;
}

int main(void)
{
    EphemerisCtx *eph = NULL;
    if (eph_load(ASSET, &eph) != CORE_OK) {
        fprintf(stderr, "oracle: cannot read %s\n", ASSET);
        fprintf(stderr, "  run from the repository root\n");
        return 1;
    }

    for (size_t b = 0; b < N_BODIES; b++) {
        for (size_t k = 0; k < N_TIMES; k++) {
            State s;
            if (eph_body_state(eph, BODIES[b], TIMES[k], &s) != CORE_OK) {
                fprintf(stderr, "oracle: body %d at t = %g failed\n",
                        BODIES[b], TIMES[k]);
                eph_free(eph);
                return 1;
            }

            printf("eph %d %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
                   BODIES[b], TIMES[k],
                   s.r.x, s.r.y, s.r.z, s.v.x, s.v.y, s.v.z);
        }
    }

    if (!propagate(eph)) {
        eph_free(eph);
        return 1;
    }

    eph_free(eph);
    return 0;
}
