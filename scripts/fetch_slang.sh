#!/bin/sh
# Вивантаження компілятора Slang (ROADMAP P1).
#
# На відміну від даних JPL, це НЕ комітиться: 24 МБ бінарників, які
# оновлюються щомісяця. Тому скрипт, а не файл у репозиторії, і тому ж
# tools/slang/ у .gitignore.
#
# Навіщо взагалі Slang — PROJECT.md §7: модулі, дженерики й один вихідний
# текст на кілька цілей замість трьох копій WGSL. Питання P1 у тому, чи
# доїжджає його вихід до wgpu, і яким саме шляхом:
#
#   через WGSL     slangc -target wgsl    → naga розбирає як звичайний шейдер
#   через SPIR-V   slangc -target spirv   → passthrough, лише де бекенд уміє
#
# Обидва перевіряє tools/slang-probe.
#
#   sh scripts/fetch_slang.sh            останній реліз
#   sh scripts/fetch_slang.sh v2026.14.1 конкретний
#
# Запускати з кореня репозиторію.

set -eu

OUT_DIR="tools/slang"
TAG="${1:-}"

# Тег закріплюється у виводі, а не лише в аргументі: інакше «у мене працює»
# й «у CI не працює» неможливо звести, бо версії різні й ніде не записані.
#
# Тег беремо з редиректу releases/latest, а НЕ з api.github.com. У API без
# токена — 60 запитів на годину на IP, і раннери macOS ділять між усіма
# джобами кілька NAT-адрес, тобто квота там регулярно вичерпана чужими
# збірками. Виглядало це як падіння рівно цього кроку за пів секунди, тоді
# як linux у тому самому прогоні проходив, а macOS за двадцять хвилин до
# того — теж. Редирект живе на github.com і такої квоти не має.
if [ -z "$TAG" ]; then
    # Статус лишається у виводі помилки навмисно. Попередня версія ловила
    # лише «тег порожній» і мовчала про причину: `curl -sS` без --fail не
    # каже про 403 нічого, тіло відповіді просто не має потрібного поля.
    RESOLVED=$(curl -sSL -o /dev/null \
               -w '%{http_code} %{url_effective}' \
               "https://github.com/shader-slang/slang/releases/latest" || true)

    case "$RESOLVED" in
        "200 "*"/releases/tag/"*) TAG=${RESOLVED##*/} ;;
        *)
            echo "не вдалося дізнатись останній тег: ${RESOLVED:-curl не відповів}" >&2
            echo "вкажіть явно, напр.: sh scripts/fetch_slang.sh v2026.14.1" >&2
            exit 1 ;;
    esac
fi

VERSION=${TAG#v}

case "$(uname -s)" in
    Linux)  OS=linux ;;
    Darwin) OS=macos ;;
    *)      echo "непідтримана ОС: $(uname -s). Windows — beрiть zip вручну" >&2
            exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64)  ARCH=x86_64 ;;
    arm64|aarch64) ARCH=aarch64 ;;
    *)             echo "непідтримана архітектура: $(uname -m)" >&2; exit 1 ;;
esac

ASSET="slang-${VERSION}-${OS}-${ARCH}.tar.gz"
URL="https://github.com/shader-slang/slang/releases/download/${TAG}/${ASSET}"

echo "Slang ${TAG}, ${OS}-${ARCH}"
echo "  $URL"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

curl -sSL --fail "$URL" | tar -xz -C "$OUT_DIR"

if [ ! -x "$OUT_DIR/bin/slangc" ]; then
    echo "у архіві немає bin/slangc — змінилась розкладка релізу?" >&2
    exit 1
fi

# Версію на диск: probe читає її й друкує в таблицю, щоб результат P1 був
# прив'язаний до конкретного компілятора, а не до «того, що стояло».
echo "$TAG" > "$OUT_DIR/VERSION"

echo ""
"$OUT_DIR/bin/slangc" -v 2>&1 | head -2 || true
echo ""
echo "Готово: $OUT_DIR/bin/slangc"
