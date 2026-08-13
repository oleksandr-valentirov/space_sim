#!/bin/sh
# Компіляція шейдерів Slang → WGSL (ROADMAP F2).
#
# Результат КОМІТИТЬСЯ, і це не лінощі, а та сама архітектура, що й з
# ассетами: кукер працює один раз на машині розробника, у білд їде результат
# (PROJECT.md §4). Інакше кожна збірка рушія вимагала б 24 МБ компілятора
# Slang, і CI теж.
#
# Побічна користь: згенерований WGSL видно в діфі. Коли дженерик розгорнеться
# не в те, це буде помітно в рев'ю, а не в кадрі.
#
# Що вихід не протух, перевіряє workflow slang-probe: він качає slangc,
# перезбирає й дивиться, чи є різниця.
#
#   sh scripts/fetch_slang.sh      один раз
#   sh scripts/build_shaders.sh
#
# Запускати з кореня репозиторію.

set -eu

SLANGC="${SLANGC:-tools/slang/bin/slangc}"
DIR="${1:-engine/shaders}"

if [ ! -x "$SLANGC" ]; then
    echo "немає $SLANGC — спершу: sh scripts/fetch_slang.sh" >&2
    exit 1
fi

found=0
for source in "$DIR"/*.slang; do
    [ -e "$source" ] || continue
    found=1
    out="${source%.slang}.wgsl"
    echo "  $source -> $out"
    "$SLANGC" "$source" -target wgsl -o "$out"
done

if [ "$found" -eq 0 ]; then
    echo "у $DIR немає жодного .slang — нічого робити, і це підозріло" >&2
    exit 1
fi

echo "Готово. Версія компілятора: $(cat tools/slang/VERSION 2>/dev/null || echo невідома)"
