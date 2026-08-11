#!/bin/sh
# «Поліція libm»: забороняє виклики libm у детермінованій зоні ядра.
#
# Чому це існує: sin/cos/exp/pow/atan2 не гарантовані бітово між платформами
# й навіть між версіями libc, тому в циклі інтегрування вони заборонені
# (PROJECT.md §4). Інваріант, який тримається лише на дисципліні, рано чи
# пізно порушується — цей скрипт робить його автоматично перевірюваним.
#
# sqrt дозволений: IEEE-754 вимагає коректного заокруглення, тож він
# однаковий скрізь.
#
# Детермінована зона — об'єктні файли верхнього рівня build/core/*.o.
# Підкаталоги (майбутній core/planning: Lambert, porkchop) свідомо НЕ
# перевіряються: планування лежить поза межею детермінізму, там libm можна.

set -eu

OBJ_DIR="${1:-build/core}"

DENY='^(sin|cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|asinh|acosh|atanh|exp|exp2|expm1|log|log2|log10|log1p|pow|cbrt|hypot|fmod|remainder|erf|erfc|lgamma|tgamma|sincos)f?l?$'

objs=$(find "$OBJ_DIR" -maxdepth 1 -name '*.o' 2>/dev/null || true)
if [ -z "$objs" ]; then
    echo "check-libm: ПОМИЛКА — не знайдено об'єктних файлів у $OBJ_DIR" >&2
    echo "  (порожня перевірка мовчки 'проходить', тому це вважається провалом)" >&2
    exit 1
fi

# nm -P — портативний формат: "symbol type value size".
# sed прибирає версію glibc (sin@GLIBC_2.2.5) і провідне підкреслення macOS.
symbols=$(
    # shellcheck disable=SC2086
    nm -P -u $objs 2>/dev/null \
        | awk '{print $1}' \
        | sed -e 's/@.*//' -e 's/^_//' \
        | sort -u
) || {
    echo "check-libm: nm недоступний або не підтримує -P." >&2
    echo "  Запасний варіант з ROADMAP A2 — перевірка за вихідним кодом." >&2
    exit 1
}

found=$(printf '%s\n' "$symbols" | grep -E "$DENY" || true)

if [ -n "$found" ]; then
    echo "check-libm: ПРОВАЛ — libm у детермінованій зоні:" >&2
    printf '  %s\n' $found >&2
    echo "" >&2
    echo "  Дозволені операції: + - * / та sqrt. Обхідні шляхи — PROJECT.md §4:" >&2
    echo "    гармоніки — рекурсії Пайнса (без тригонометрії)" >&2
    echo "    обертання тіл — чебишевські поліноми з ассета" >&2
    echo "    атмосфера — таблиця густини з поліноміальною інтерполяцією" >&2
    exit 1
fi

count=$(printf '%s\n' "$symbols" | grep -c . || true)
echo "check-libm: чисто (перевірено невизначених символів: $count)"
