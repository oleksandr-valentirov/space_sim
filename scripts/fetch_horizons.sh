#!/bin/sh
# Вивантаження еталонних даних з JPL Horizons (ROADMAP B1).
#
# Ці дані — оракул, відносно якого вимірюється власний інтегратор. Писати
# інтегратор, не маючи з чим порівняти, — найдорожчий спосіб рухатись, тому
# крок іде до B3, а не після нього.
#
# Результат комітиться в репозиторій: дані статичні, повторно вивантажувати
# не треба. Скрипт існує заради відтворюваності й документування запиту,
# а не для регулярного запуску.
#
# ВАЖЛИВО — параметри запиту є частиною контракту даних:
#   CENTER='500@0'   барицентр Сонячної системи
#   REF_PLANE='FRAME' + REF_SYSTEM='ICRF'   екваторіальна ICRF, НЕ екліптика
#   OUT_UNITS='KM-S' км і км/с — у метри конвертуємо при імпорті (vec3.h)
#   VEC_CORR='NONE'  геометричні вектори, без світлової затримки й аберації
#   Час — JDTDB (барицентричний динамічний), не UTC: без високосних секунд.
#
# Зміна будь-чого з цього робить дані несумісними з попередніми. Саме тут
# народжується більшість розбіжностей у чисельних задачах — не у фізиці.

set -eu

OUT_DIR="${1:-data/horizons}"
API="https://ssd.jpl.nasa.gov/api/horizons.api"

START="2000-01-01 12:00"
STOP="2010-01-01 12:00"
STEP="30d"

# id:назва:obj_id
#   id     — для векторів
#   obj_id — для параметрів тіла; відрізняється у барицентрів, бо барицентр
#            це динамічна точка й GM у нього не публікується
#
# Сонце, Земля, Місяць — для M0. Венера, Марс, Юпітер — для гілки розвилки B5
# («додавайте збурення по одному»), щоб не ходити по них двічі.
BODIES="10:sun:10 399:earth:399 301:moon:301 299:venus:299 4:mars_bary:499 5:jupiter_bary:599"

mkdir -p "$OUT_DIR"

fetch_vectors() {
    id="$1"; name="$2"
    echo "  вектори: $name ($id)"
    curl -sS -G "$API" \
        --data-urlencode "format=text" \
        --data-urlencode "COMMAND='$id'" \
        --data-urlencode "OBJ_DATA='NO'" \
        --data-urlencode "MAKE_EPHEM='YES'" \
        --data-urlencode "EPHEM_TYPE='VECTORS'" \
        --data-urlencode "CENTER='500@0'" \
        --data-urlencode "START_TIME='$START'" \
        --data-urlencode "STOP_TIME='$STOP'" \
        --data-urlencode "STEP_SIZE='$STEP'" \
        --data-urlencode "REF_PLANE='FRAME'" \
        --data-urlencode "REF_SYSTEM='ICRF'" \
        --data-urlencode "OUT_UNITS='KM-S'" \
        --data-urlencode "VEC_TABLE='2'" \
        --data-urlencode "VEC_LABELS='NO'" \
        --data-urlencode "VEC_CORR='NONE'" \
        --data-urlencode "CSV_FORMAT='YES'" \
        > "$OUT_DIR/.raw_$name.txt"

    if ! grep -q '\$\$SOE' "$OUT_DIR/.raw_$name.txt"; then
        echo "ПОМИЛКА: Horizons не повернув таблицю для $name ($id)" >&2
        head -20 "$OUT_DIR/.raw_$name.txt" >&2
        exit 1
    fi

    {
        echo "# JPL Horizons, $name (COMMAND=$id)"
        echo "# center=SSB(500@0) frame=ICRF/FRAME units=KM-S corr=NONE"
        echo "# jdtdb,x_km,y_km,z_km,vx_kms,vy_kms,vz_kms"
        sed -n '/\$\$SOE/,/\$\$EOE/p' "$OUT_DIR/.raw_$name.txt" \
            | sed -e '/\$\$SOE/d' -e '/\$\$EOE/d' \
            | awk -F', *' '{printf "%s,%s,%s,%s,%s,%s,%s\n", $1,$3,$4,$5,$6,$7,$8}'
    } > "$OUT_DIR/vec_$name.csv"

    rm -f "$OUT_DIR/.raw_$name.txt"
}

# Гравітаційні параметри беремо з того самого джерела, що й вектори, а не
# з пам'яті чи підручника: інакше сила й еталон рахуються за різними GM,
# і розбіжність спишеться на інтегратор.
fetch_object_data() {
    id="$1"; name="$2"
    echo "  параметри тіла: $name ($id)"
    curl -sS -G "$API" \
        --data-urlencode "format=text" \
        --data-urlencode "COMMAND='$id'" \
        --data-urlencode "OBJ_DATA='YES'" \
        --data-urlencode "MAKE_EPHEM='NO'" \
        | grep -v '^Ephemeris / API_USER' \
        > "$OUT_DIR/obj_$name.txt"
}

echo "Вивантаження з JPL Horizons -> $OUT_DIR"
echo "  інтервал: $START .. $STOP, крок $STEP"

for entry in $BODIES; do
    id=$(echo "$entry" | cut -d: -f1)
    name=$(echo "$entry" | cut -d: -f2)
    obj_id=$(echo "$entry" | cut -d: -f3)
    fetch_vectors "$id" "$name"
    fetch_object_data "$obj_id" "$name"
done

# Витягуємо GM у машиночитний вигляд.
#
# Витягуємо саме присвоєння «GM ... = число», а не рядок, у якому воно
# трапилось. Причина конкретна: у Місяця GM стоїть посеред рядка з радіусом
#   «Radius (IAU), km = 1737.4    GM, km^3/s^2 = 4902.800066»
# і будь-яка обробка «взяти число після першого =» дала б радіус.
#
# Рядок «GM 1-sigma» відсіюється самою формою шаблону: після GM має йти
# одиниця виміру, а не «1-sigma».
{
    echo "# Гравітаційні параметри з JPL Horizons, км^3/с^2."
    echo "# Джерело — obj_<name>.txt у цьому ж каталозі."
    echo "# УВАГА: у mars_bary і jupiter_bary це GM ПЛАНЕТИ, не системи."
    echo "# Див. README.md, розділ про GM барицентрів."
    echo "# name,gm_km3_s2"
    for entry in $BODIES; do
        name=$(echo "$entry" | cut -d: -f2)
        gm=$(grep -oiE 'GM[ ,]*\(?km\^3/s\^2\)?[ ,]*=[[:space:]]*[0-9.]+' \
                "$OUT_DIR/obj_$name.txt" \
             | head -1 \
             | sed -E 's/.*=[[:space:]]*//')
        if [ -z "$gm" ]; then
            echo "ПОМИЛКА: не знайдено GM для $name" >&2
            exit 1
        fi
        echo "$name,$gm"
    done
} > "$OUT_DIR/gm.csv"

echo "Готово. Рядків у таблицях:"
for entry in $BODIES; do
    name=$(echo "$entry" | cut -d: -f2)
    printf "  %-14s %s\n" "$name" "$(grep -vc '^#' "$OUT_DIR/vec_$name.csv")"
done
echo "GM:"
grep -v '^#' "$OUT_DIR/gm.csv" | sed 's/^/  /'
