# Збірка числового ядра на C. Milestone 0 — Rust тут не бере участі.
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
#   make clean

CC ?= cc
AR ?= ar

# Прапорці беруться з core/cflags.txt і НІДЕ більше не задаються.
CFLAGS := $(shell sed -e 's/#.*//' core/cflags.txt | tr '\n' ' ')

# Дві бібліотеки — це і є межа детермінізму, виражена в графі збірки:
#
#   libcore.a          core/*.c        РАНТАЙМ. libm заборонений, -lm не
#                                      лінкується взагалі. Другий рубіж
#                                      захисту після make check-libm.
#   libcore_offline.a  core/offline/*.c КУКЕР. libm дозволений, лінкується
#                                      з -lm. Сюди йде все, що рахується
#                                      наперед і потрапляє в білд ассетом.
#
# Сценарії детермінізму лінкуються ТІЛЬКИ з libcore.a і без -lm: якщо туди
# просочиться тригонометрія, лінкування впаде. Тести лінкуються з обома.
LDLIBS :=
LDLIBS_OFFLINE := -lm

BUILD := build
LIB   := $(BUILD)/libcore.a
LIB_OFFLINE := $(BUILD)/libcore_offline.a

CORE_SRC := $(sort $(wildcard core/*.c))
CORE_OBJ := $(patsubst core/%.c,$(BUILD)/core/%.o,$(CORE_SRC))

OFFLINE_SRC := $(sort $(wildcard core/offline/*.c))
OFFLINE_OBJ := $(patsubst core/offline/%.c,$(BUILD)/core/offline/%.o,$(OFFLINE_SRC))

# $(sort) не для краси: порядок сценаріїв визначає порядок рядків у actual.txt,
# а $(wildcard) не гарантує сталого порядку. Без сортування звірка з еталоном
# могла б падати через перестановку рядків.
TEST_SRC := $(sort $(wildcard core/test/*.c))
TEST_BIN := $(patsubst core/test/%.c,$(BUILD)/test/%,$(TEST_SRC))

COOK_SRC := $(sort $(wildcard core/cook/*.c))
COOK_BIN := $(patsubst core/cook/%.c,$(BUILD)/cook/%,$(COOK_SRC))

SCEN_SRC := $(sort $(wildcard core/scenario/*.c))
SCEN_BIN := $(patsubst core/scenario/%.c,$(BUILD)/scenario/%,$(SCEN_SRC))
GOLDEN   := core/scenario/golden.txt
ACTUAL   := $(BUILD)/scenario/actual.txt

.PHONY: all test unit check-libm determinism determinism-bless hashes cook flags clean

all: $(LIB) $(LIB_OFFLINE)

$(BUILD)/core/%.o: core/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -c $< -o $@

$(BUILD)/core/offline/%.o: core/offline/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -Icore/offline -c $< -o $@

$(LIB): $(CORE_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(LIB_OFFLINE): $(OFFLINE_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(BUILD)/test/%: core/test/%.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -Icore/offline -o $@ $< \
		$(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# Кукер: офлайновий, libm дозволений.
$(BUILD)/cook/%: core/cook/%.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -Icore/offline -o $@ $< \
		$(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# Без libcore_offline.a і без -lm: лінкування тут — жива перевірка того,
# що в рантаймовій частині немає libm.
$(BUILD)/scenario/%: core/scenario/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

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

$(ACTUAL): $(SCEN_BIN)
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
	@$(BUILD)/cook/cook_fixture
	@echo ""
	@echo "Перегенеровано. Перевірте git diff і що визначає зміну:"
	@echo "  зміна тут змінює всі хеші сценаріїв, які читають ассет."

flags:
	@echo $(CFLAGS)

clean:
	rm -rf $(BUILD)
