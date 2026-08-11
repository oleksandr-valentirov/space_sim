# Збірка числового ядра на C. Milestone 0 — Rust тут не бере участі.
# Послідовність робіт: ROADMAP.md, крок A1.
#
#   make        зібрати статичну бібліотеку
#   make test   зібрати й прогнати юніт-тести
#   make flags  показати фактичні прапорці (для звірки з build.rs на M1)
#   make clean

CC ?= cc
AR ?= ar

# Прапорці беруться з core/cflags.txt і НІДЕ більше не задаються.
CFLAGS := $(shell sed -e 's/#.*//' core/cflags.txt | tr '\n' ' ')

# -lm свідомо НЕ лінкуємо: у циклі інтегрування libm заборонений (PROJECT.md §4).
# Якщо лінкування колись впаде на sqrt — це не привід додати -lm, а привід
# додати -fno-math-errno у cflags.txt (на бітову точність не впливає).
LDLIBS :=

BUILD := build
LIB   := $(BUILD)/libcore.a

CORE_SRC := $(wildcard core/*.c)
CORE_OBJ := $(patsubst core/%.c,$(BUILD)/core/%.o,$(CORE_SRC))

TEST_SRC := $(wildcard core/test/*.c)
TEST_BIN := $(patsubst core/test/%.c,$(BUILD)/test/%,$(TEST_SRC))

.PHONY: all test clean flags

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

test: $(TEST_BIN)
	@fail=0; \
	for t in $(TEST_BIN); do \
		echo "== $$t"; \
		$$t || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "TESTS FAILED"; exit 1; fi; \
	echo "all tests passed"

flags:
	@echo $(CFLAGS)

clean:
	rm -rf $(BUILD)
