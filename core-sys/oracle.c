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
 * Формат рядка: <body> <t> <x> <y> <z> <vx> <vy> <vz>
 *
 * Запускається з кореня репозиторію. */

#include "ephemeris.h"

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

            printf("%d %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n",
                   BODIES[b], TIMES[k],
                   s.r.x, s.r.y, s.r.z, s.v.x, s.v.y, s.v.z);
        }
    }

    eph_free(eph);
    return 0;
}
