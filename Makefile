# Збірка числового ядра на C. Milestone 0 — Rust тут не бере участі.
# Послідовність робіт: ROADMAP.md.
#
#   make                  зібрати статичну бібліотеку
#   make test             усі перевірки: libm, юніт-тести, детермінізм
#   make unit             лише юніт-тести
#   make check-libm       лише «поліція libm» (ROADMAP A2)
#   make determinism      звірити хеші сценаріїв з еталонними
#   make determinism-bless оновити еталонні хеші (робити свідомо!)
#   make flags            показати фактичні прапорці (звірка з build.rs на M1)
#   make clean

CC ?= cc
AR ?= ar

# Прапорці беруться з core/cflags.txt і НІДЕ більше не задаються.
CFLAGS := $(shell sed -e 's/#.*//' core/cflags.txt | tr '\n' ' ')

# -lm свідомо НЕ лінкуємо: у детермінованій зоні libm заборонений
# (PROJECT.md §4). Це другий рубіж захисту після make check-libm.
LDLIBS :=

BUILD := build
LIB   := $(BUILD)/libcore.a

CORE_SRC := $(wildcard core/*.c)
CORE_OBJ := $(patsubst core/%.c,$(BUILD)/core/%.o,$(CORE_SRC))

TEST_SRC := $(wildcard core/test/*.c)
TEST_BIN := $(patsubst core/test/%.c,$(BUILD)/test/%,$(TEST_SRC))

SCEN_SRC := $(wildcard core/scenario/*.c)
SCEN_BIN := $(patsubst core/scenario/%.c,$(BUILD)/scenario/%,$(SCEN_SRC))
GOLDEN   := core/scenario/golden.txt
ACTUAL   := $(BUILD)/scenario/actual.txt

.PHONY: all test unit check-libm determinism determinism-bless flags clean

all: $(LIB)

$(BUILD)/core/%.o: core/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -c $< -o $@

$(LIB): $(CORE_OBJ)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(BUILD)/test/%: core/test/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

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

flags:
	@echo $(CFLAGS)

clean:
	rm -rf $(BUILD)
