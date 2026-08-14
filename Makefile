# Збірка числового ядра на C.
#
# З M1 ті самі .c збирає ще й `cargo` через core-sys/build.rs (ROADMAP D1).
# Прапорці обидві збірки читають з core/cflags.txt і більше нізвідки, а що
# вони дають однакові числа — перевіряє core-sys/tests/determinism.rs проти
# того самого core/scenario/golden.txt. Звірити руками:
#
#     make flags
#     cargo run -q --example flags
#
# Послідовність робіт: ROADMAP.md.
#
#   make                  зібрати статичну бібліотеку
#   make test             усі перевірки: libm, юніт-тести, детермінізм
#   make unit             лише юніт-тести
#   make check-libm       лише «поліція libm» (ROADMAP A2)
#   make determinism      звірити хеші сценаріїв з еталонними
#   make determinism-bless оновити еталонні хеші (робити свідомо!)
#   make hashes           показати фактичні хеші сценаріїв
#   make flags            показати фактичні прапорці (звірка з build.rs на M1)
#   make cook             перегенерувати ассет-фікстуру (робити свідомо!)
#   make cook ANCHOR_BARYCENTRE=0   те саме, без закріплення баріцентру —
#                         лише щоб зміряти ефект, у гру їде анкерований ассет
#   make csv              вивести результати ядра у build/csv/*.csv
#   make plots            побудувати графіки з CSV у build/plots/*.png
#   make bench            пропускна здатність DOP853 (скіл perf-probe)
#   make clean

CC ?= cc
AR ?= ar

# Прапорці беруться з core/cflags.txt і НІДЕ більше не задаються.
#
# HASH виглядає безглуздо, але без нього Makefile залежить від версії make.
# Коментарі знімаються ДО розбору функцій, тож `#` усередині sed-програми
# make бачить як початок коментаря й обрізає рядок — `$(shell` лишається
# незакритим. GNU make 4.3 це пробачає, 3.81 (той, що на macOS) падає з
# "unterminated call to function `shell'". Через змінну `#` доходить до
# оболонки вже після розбору.
HASH := \#

# LC_ALL=C не для краси: cflags.txt має коментарі українською, а BSD sed
# (macOS) у не-UTF-8 локалі падає на багатобайтових послідовностях з
# "illegal byte sequence". У локалі C він працює з байтами і йому байдуже.
CFLAGS := $(shell LC_ALL=C sed -e 's/$(HASH).*//' core/cflags.txt | tr '\n' ' ')

# А це — те, чого бракувало. $(shell) не повідомляє про помилку: якщо sed
# впав, CFLAGS просто порожні, і збірка тихо піде з дефолтними прапорцями
# компілятора. Тобто БЕЗ -ffp-contract=off — і детермінізм зламається без
# жодного повідомлення, а впаде потім звірка хешів, за кілометр від причини.
#
# Перевіряємо не лише «щось є», а конкретний прапорець: порожній рядок ловить
# зламаний sed, а відсутність саме -ffp-contract=off ловить ще й правку
# cflags.txt, яка забирає його не подумавши.
ifeq ($(strip $(CFLAGS)),)
$(error Прапорці не витягнулися з core/cflags.txt. Збірка з дефолтними \
прапорцями порушила б детермінізм, тому це помилка, а не попередження.)
endif
ifeq (,$(findstring -ffp-contract=off,$(CFLAGS)))
$(error У прапорцях немає -ffp-contract=off. Без нього компілятор зливає \
множення й додавання у FMA, і той самий код дає різні біти на різних \
платформах — PROJECT.md §4.)
endif

# Залежності від заголовків. НЕ впливають на арифметику: -MMD -MP лише
# просять компілятор виписати побічний файл .d зі списком включених
# заголовків, кодогенерацію вони не чіпають. Тому їх тут, а не в
# core/cflags.txt — той файл лишається єдиним джерелом прапорців, які
# визначають числа.
#
# Навіщо: без цього зміна .h не перезбирала нічого, бо в правилах стояли
# лише .c. Спіймано на ROADMAP K4 — у FieldCtx додалося поле, field.c
# перезібрався, prop.c ні, і два об'єктні файли розійшлися в sizeof тієї
# самої структури. Це не дало неправильних чисел, це зруйнувало купу
# (malloc(): invalid size), тобто найгучніший з можливих проявів; тихий
# прояв тієї ж помилки — трохи інші числа — був би незрівнянно гіршим.
#
# Той самий клас діри, що вже описаний у ROADMAP D1 для watch() у
# build.rs, і з тією ж мораллю: перевірка, яка існує заради ловіння тихих
# змін, сама мусить бачити всі свої входи.
DEPFLAGS := -MMD -MP

# Три бібліотеки — це і є межа детермінізму, виражена в графі збірки:
#
#   libcore.a           core/*.c          РАНТАЙМ, пропагація. libm
#                                         заборонений, -lm не лінкується
#                                         взагалі. Другий рубіж захисту
#                                         після make check-libm.
#   libcore_offline.a   core/offline/*.c  КУКЕР. libm дозволений, лінкується
#                                         з -lm. Не рантайм: рахується раз на
#                                         машині розробника, у гру їде ассет.
#   libcore_planning.a  core/planning/*.c РАНТАЙМ, планування. libm
#                                         дозволений (PROJECT.md §4: межа
#                                         детермінізму — по пропагації, не по
#                                         плануванню). scripts/check_no_libm.sh
#                                         свідомо перевіряє лише build/core
#                                         верхнього рівня, тому цей підкаталог
#                                         під поліцію libm не підпадає.
#
# Сценарії детермінізму лінкуються ТІЛЬКИ з libcore.a і без -lm: якщо туди
# просочиться тригонометрія, лінкування впаде. Тести лінкуються з усіма
# трьома.
LDLIBS :=
LDLIBS_OFFLINE := -lm
LDLIBS_PLANNING := -lm

# MinGW дописує .exe до виконуваних файлів незалежно від -o, тож без цього
# make вважав би цілі непобудованими й перезбирав усе щоразу. MSYS2 успадковує
# OS=Windows_NT з Windows, тож перевірка надійна (ROADMAP C5).
EXE :=
ifeq ($(OS),Windows_NT)
EXE := .exe
endif

# Прибирання залишкового імпульсу в кукері (nbody_anchor_barycentre).
# Типово увімкнено; вимикається лише щоб зміряти власний ефект:
#
#     make cook                        як їде в гру
#     make cook ANCHOR_BARYCENTRE=0    без прибирання, для порівняння
#
# Це ЄДИНЕ, що цією змінною можна передати в компілятор, і значення
# перевіряється нижче. Загального EXTRA_CFLAGS тут немає навмисно: він був би
# дірою, крізь яку в збірку заходить -ffast-math, а прапорці мають лишатися
# в core/cflags.txt і більше ніде.
ANCHOR_BARYCENTRE ?= 1
ifeq (,$(filter 0 1,$(ANCHOR_BARYCENTRE)))
$(error ANCHOR_BARYCENTRE має бути 0 або 1, а не «$(ANCHOR_BARYCENTRE)»)
endif
OFFLINE_DEFS := -DEPH_ANCHOR_BARYCENTRE=$(ANCHOR_BARYCENTRE)

BUILD := build
LIB   := $(BUILD)/libcore.a
LIB_OFFLINE := $(BUILD)/libcore_offline.a
LIB_PLANNING := $(BUILD)/libcore_planning.a

CORE_SRC := $(sort $(wildcard core/*.c))
CORE_OBJ := $(patsubst core/%.c,$(BUILD)/core/%.o,$(CORE_SRC))

OFFLINE_SRC := $(sort $(wildcard core/offline/*.c))
OFFLINE_OBJ := $(patsubst core/offline/%.c,$(BUILD)/core/offline/%.o,$(OFFLINE_SRC))
ANCHOR_STAMP := $(BUILD)/core/offline/.anchor-$(ANCHOR_BARYCENTRE)

PLANNING_SRC := $(sort $(wildcard core/planning/*.c))
PLANNING_OBJ := $(patsubst core/planning/%.c,$(BUILD)/core/planning/%.o,$(PLANNING_SRC))

# $(sort) не для краси: порядок сценаріїв визначає порядок рядків у actual.txt,
# а $(wildcard) не гарантує сталого порядку. Без сортування звірка з еталоном
# могла б падати через перестановку рядків.
TEST_SRC := $(sort $(wildcard core/test/*.c))
TEST_BIN := $(patsubst core/test/%.c,$(BUILD)/test/%$(EXE),$(TEST_SRC))

COOK_SRC := $(sort $(wildcard core/cook/*.c))
COOK_BIN := $(patsubst core/cook/%.c,$(BUILD)/cook/%$(EXE),$(COOK_SRC))

# csv.c is the shared writer, not a program: it is compiled into each exporter
# rather than being one, so it must not match the pattern that builds them.
EXPORT_SRC := $(filter-out core/export/csv.c,$(sort $(wildcard core/export/*.c)))
EXPORT_BIN := $(patsubst core/export/%.c,$(BUILD)/export/%$(EXE),$(EXPORT_SRC))
CSV_DIR    := $(BUILD)/csv
PLOT_DIR   := $(BUILD)/plots

BENCH_SRC := $(sort $(wildcard core/bench/*.c))
BENCH_BIN := $(patsubst core/bench/%.c,$(BUILD)/bench/%$(EXE),$(BENCH_SRC))

PYTHON ?= python3

SCEN_SRC := $(sort $(wildcard core/scenario/*.c))
SCEN_BIN := $(patsubst core/scenario/%.c,$(BUILD)/scenario/%$(EXE),$(SCEN_SRC))
GOLDEN   := core/scenario/golden.txt
ACTUAL   := $(BUILD)/scenario/actual.txt

# Ассет — такий самий вхід сценаріїв, як їхній власний код: sc_ephemeris
# і sc_trajectory читають його в рантаймі. Без цієї залежності `make cook`
# змінював ассет, а наступний `make test` звіряв СТАРИЙ actual.txt і мовчки
# проходив — тобто перевірка, яка існує заради ловіння тихих змін, сама
# пропускала б найтихішу з них.
FIXTURE  := $(wildcard data/fixture/*.eph)

# Файли залежностей, які виписав -MMD. Кожен перелічує заголовки, від яких
# залежить його ціль, у синтаксисі make. `-include` мовчить, коли їх ще
# немає (перша збірка), а -MP додає фіктивні цілі для самих заголовків, щоб
# видалення чи перейменування .h не ламало збірку помилкою «немає правила».
DEP := $(CORE_OBJ:.o=.d) $(OFFLINE_OBJ:.o=.d) $(PLANNING_OBJ:.o=.d) \
       $(patsubst core/test/%.c,$(BUILD)/test/%.d,$(TEST_SRC)) \
       $(patsubst core/cook/%.c,$(BUILD)/cook/%.d,$(COOK_SRC)) \
       $(patsubst core/export/%.c,$(BUILD)/export/%.d,$(EXPORT_SRC)) \
       $(patsubst core/bench/%.c,$(BUILD)/bench/%.d,$(BENCH_SRC)) \
       $(patsubst core/scenario/%.c,$(BUILD)/scenario/%.d,$(SCEN_SRC))

-include $(DEP)

.PHONY: all test unit check-libm determinism determinism-bless hashes cook \
        csv plots bench flags clean

all: $(LIB) $(LIB_OFFLINE) $(LIB_PLANNING)

$(BUILD)/core/%.o: core/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -c $< -o $@

$(BUILD)/core/offline/%.o: core/offline/%.c $(ANCHOR_STAMP)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) $(OFFLINE_DEFS) -Icore -Icore/offline -c $< -o $@

# Без цього `make cook ANCHOR_BARYCENTRE=0` після звичайного `make` нічого б
# не перезібрав: make не бачить значень змінних, лише файли. Ім'я штампа
# несе значення, тож зміна значення робить його неіснуючим — і всі об'єктні
# файли кукера стають застарілими. Мовчазний ассет, скукований не тим кодом,
# який просили, — рівно той клас помилки, який ловить решта цього файлу.
$(ANCHOR_STAMP):
	@mkdir -p $(dir $@)
	@rm -f $(BUILD)/core/offline/.anchor-*
	@touch $@

$(BUILD)/core/planning/%.o: core/planning/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/planning -c $< -o $@

$(LIB): $(CORE_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(LIB_OFFLINE): $(OFFLINE_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(LIB_PLANNING): $(PLANNING_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(BUILD)/test/%$(EXE): core/test/%.c $(LIB) $(LIB_OFFLINE) $(LIB_PLANNING)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/offline -Icore/planning -o $@ $< \
		$(LIB_OFFLINE) $(LIB_PLANNING) $(LIB) $(LDLIBS_OFFLINE)

# Кукер: офлайновий, libm дозволений.
$(BUILD)/cook/%$(EXE): core/cook/%.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/offline -o $@ $< \
		$(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# Експортери CSV. Лінкуються як тести, з обома бібліотеками й -lm: це
# діагностичні драйвери, а не рантайм, і один з них (ex_horizons) свідомо
# ганяє офлайновий взаємний N-body проти Horizons. Живу перевірку «в рантаймі
# немає libm» дають сценарії нижче — дублювати її тут нічого не додає.
$(BUILD)/export/%$(EXE): core/export/%.c core/export/csv.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/offline -Icore/export -o $@ \
		$< core/export/csv.c $(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# Без libcore_offline.a і без -lm: лінкування тут — жива перевірка того,
# що в рантаймовій частині немає libm.
$(BUILD)/scenario/%$(EXE): core/scenario/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

# Той самий рантаймовий libcore.a, без -lm: бенчмарк заявляє, що міряє
# пропускну здатність деталізованої фізики (CLAUDE.md, інваріант 3 — жодного
# libm у циклі інтегрування), і лінкування без -lm — жива перевірка цього,
# а не просто оптимізм.
$(BUILD)/bench/%$(EXE): core/bench/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

# --- Перевірки -------------------------------------------------------------

# Порядок навмисний: спершу найдешевша й найконкретніша перевірка.
test: check-libm unit determinism
	@echo ""
	@echo "УСІ ПЕРЕВІРКИ ПРОЙДЕНІ"

check-libm: $(LIB)
	@sh scripts/check_no_libm.sh $(BUILD)/core

unit: $(TEST_BIN)
	@fail=0; \
	for t in $(TEST_BIN); do \
		echo "== $$t"; \
		$$t || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "ЮНІТ-ТЕСТИ ПРОВАЛЕНІ"; exit 1; fi

$(ACTUAL): $(SCEN_BIN) $(FIXTURE)
	@mkdir -p $(dir $@)
	@rm -f $@
	@for s in $(SCEN_BIN); do $$s >> $@; done

determinism: $(ACTUAL)
	@if [ ! -f $(GOLDEN) ]; then \
		echo "determinism: еталонів немає — спершу make determinism-bless" >&2; \
		exit 1; \
	fi
	@if diff -u $(GOLDEN) $(ACTUAL) > $(BUILD)/scenario/diff.txt; then \
		echo "determinism: хеші збігаються з еталонними"; \
	else \
		echo "determinism: ПРОВАЛ — хеші розійшлися з $(GOLDEN)" >&2; \
		cat $(BUILD)/scenario/diff.txt >&2; \
		echo "" >&2; \
		echo "  Якщо зміна поведінки навмисна — make determinism-bless" >&2; \
		echo "  і покажіть різницю в коміті. Якщо ні — це регресія." >&2; \
		exit 1; \
	fi

determinism-bless: $(ACTUAL)
	@cp $(ACTUAL) $(GOLDEN)
	@echo "Еталонні хеші оновлено. Перегляньте git diff $(GOLDEN) перед комітом:"
	@echo "  зміна тут означає, що результат симуляції змінився."

# Для ручної звірки між машинами, коли CI ще немає або коли він уже впав
# і треба подивитись очима (ROADMAP C5).
hashes: $(ACTUAL)
	@cat $(ACTUAL)

# Ассет-фікстура закомічена навмисно. Кукер використовує cos() через
# чебишевську підгонку, тож два комп'ютери порахували б РІЗНІ коефіцієнти —
# і крос-платформна звірка падала б на кукері, а не на рантаймі, який вона
# має перевіряти. Готуємо один раз, комітимо, звіряємо (ROADMAP C5).
cook: $(COOK_BIN)
	@mkdir -p data/fixture
	@$(BUILD)/cook/cook_fixture$(EXE)
	@echo ""
	@echo "Перегенеровано. Перевірте git diff і що визначає зміну:"
	@echo "  зміна тут змінює всі хеші сценаріїв, які читають ассет."

# --- Поставка M0: подивитися очима ----------------------------------------
#
# Тести кажуть «пройдено», а це показує, що саме пораховано. Обидва потрібні:
# траєкторія, яка вкладається в допуск і при цьому летить не туди, — річ, яка
# трапляється, і ловиться вона оком, а не порогом.
#
# CSV — артефакти збірки, у git їх немає (.gitignore). Вони НЕ входять у
# звірку детермінізму: друк double у десятковий текст — справа libc, а не
# IEEE, тож порівнювати ці файли між платформами не можна. Для цього є хеші
# сценаріїв (core/export/csv.h).
csv: $(EXPORT_BIN)
	@mkdir -p $(CSV_DIR)
	@for e in $(EXPORT_BIN); do echo "== $$e"; $$e || exit 1; done
	@echo ""
	@echo "CSV у $(CSV_DIR). Далі: make plots"

# matplotlib свідомо не є залежністю збірки: ядро від нього не залежить,
# і на CI його немає. Скрипт сам скаже, чого бракує.
plots: csv
	@mkdir -p $(PLOT_DIR)
	@$(PYTHON) scripts/plot.py --csv $(CSV_DIR) --out $(PLOT_DIR)

# Пропускна здатність DOP853 на цій машині, у число, не хеш (скіл
# perf-probe). Час стінного годинника нестабільний між прогонами й
# машинами навмисно — саме тому результат не входить у determinism.
bench: $(BENCH_BIN)
	@for b in $(BENCH_BIN); do echo "== $$b"; $$b; done

flags:
	@echo $(CFLAGS)

clean:
	rm -rf $(BUILD)
