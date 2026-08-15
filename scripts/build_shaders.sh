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

    # ⚠ Одна виправка після компілятора, і вона не косметична.
    #
    # Slang друкує bindless-масив текстур як `array<texture_2d<...>>`, а
    # `naga` такого не приймає взагалі: масив ресурсів у WGSL має власне
    # ключове слово `binding_array`, і без нього модуль падає на
    # `create_shader_module` з «Base type for the array is invalid».
    #
    # Це той самий клас розбіжності, що вже змусив брати WGSL замість SPIR-V
    # (capability DrawParameters, ROADMAP F2): два інструменти розуміють одну
    # специфікацію трохи по-різному. Заміна вузька навмисно — вона чіпає лише
    # оголошення `var ... : array<texture_...>`, тобто рівно ту конструкцію,
    # якої в WGSL не існує в такому вигляді.
    sed -i.bak -E \
        's/(var [A-Za-z0-9_]+ : )array<(texture_[a-z0-9_]+<[^>]*>)>/\1binding_array<\2>/' \
        "$out"
    rm -f "$out.bak"

    # І, якщо масив таки з'явився, — вмикач розширення першим рядком файлу.
    # `binding_array` у WGSL це розширення wgpu, а не сама специфікація, тож
    # без `enable` модуль падає з прямою вказівкою, чого бракує. Дописується
    # тут, а не руками у .wgsl: згенерований файл ніхто не редагує.
    if grep -q "binding_array<" "$out"; then
        printf 'enable wgpu_binding_array;\n\n%s' "$(cat "$out")" > "$out.tmp"
        mv "$out.tmp" "$out"
    fi
done

if [ "$found" -eq 0 ]; then
    echo "у $DIR немає жодного .slang — нічого робити, і це підозріло" >&2
    exit 1
fi

echo "Готово. Версія компілятора: $(cat tools/slang/VERSION 2>/dev/null || echo невідома)"
