# Build of the C numeric core.
#
# Since M1 the same .c files are also built by `cargo` via core-sys/build.rs
# (ROADMAP D1). Both builds read their flags from core/cflags.txt and nowhere
# else; that they produce identical numbers is checked by
# core-sys/tests/determinism.rs against the same core/scenario/golden.txt.
# To compare by hand:
#
#     make flags
#     cargo run -q --example flags
#
# Order of work: ROADMAP.md.
#
#   make                  build the static library
#   make test             all checks: libm, unit tests, determinism
#   make unit             unit tests only
#   make asan             the same unit tests under ASan+UBSan (memory errors)
#   make valgrind         the same unit tests under valgrind (uninitialised
#                         memory -- which ASan does not see)
#   make coverage         line coverage of the C sources by the unit tests
#                         (the README badge; not a gate)
#   make check-libm       the "libm police" only (ROADMAP A2)
#   make determinism      compare scenario hashes against the golden file
#   make determinism-bless update the golden hashes (do this deliberately!)
#   make hashes           print the actual scenario hashes
#   make flags            print the actual flags (compare with build.rs on M1)
#   make cook             regenerate the asset fixture (do this deliberately!)
#   make cook-dem         cook terrain tiles from data/lola into /assets/
#   make cook-colour      cook colour tiles from data/wac into /assets/
#   make cook-stars       cook the star catalogue from data/bsc5 into /assets/
#   make model-ship       rebuild the ship model in Blender (writes to git!)
#   make cook-ship        cook the ship from assets-src/ into /assets/
#   make cook ANCHOR_BARYCENTRE=0   the same without barycentre anchoring --
#                         only to measure the effect; the game ships anchored
#   make csv              export core results to build/csv/*.csv
#   make plots            plot the CSV into build/plots/*.png
#   make bench            DOP853 throughput (skill perf-probe)
#   make clean

CC ?= cc
AR ?= ar

# Flags come from core/cflags.txt and are set NOWHERE else.
#
# HASH looks pointless but without it the Makefile depends on the make version.
# Comments are stripped BEFORE functions are parsed, so a `#` inside the sed
# program reads as the start of a comment and truncates the line, leaving
# `$(shell` unclosed. GNU make 4.3 forgives that, 3.81 (the one on macOS) dies
# with "unterminated call to function `shell'". Through a variable the `#`
# reaches the shell after parsing.
HASH := \#

# LC_ALL=C keeps sed byte-oriented whatever lands in cflags.txt: BSD sed
# (macOS) in a non-UTF-8 locale dies on multibyte sequences with "illegal byte
# sequence". In the C locale it does not care.
CFLAGS := $(shell LC_ALL=C sed -e 's/$(HASH).*//' core/cflags.txt | tr '\n' ' ')

# $(shell) reports no error: if sed fails, CFLAGS is simply empty and the
# build quietly proceeds with the compiler's default flags -- that is, WITHOUT
# -ffp-contract=off, so determinism breaks silently and the hash comparison
# fails later, a mile from the cause.
#
# We check not just "something is there" but the specific flag: an empty string
# catches broken sed, and a missing -ffp-contract=off also catches an edit to
# cflags.txt that drops it without thinking.
ifeq ($(strip $(CFLAGS)),)
$(error Could not extract flags from core/cflags.txt. Building with default \
flags would break determinism, so this is an error, not a warning.)
endif
ifeq (,$(findstring -ffp-contract=off,$(CFLAGS)))
$(error Flags do not contain -ffp-contract=off. Without it the compiler \
fuses multiply and add into FMA, and the same code gives different bits on \
different platforms -- PROJECT.md section 4.)
endif

# Header dependencies. They do NOT affect arithmetic: -MMD -MP only ask the
# compiler to write a side .d file listing included headers, leaving codegen
# alone. Hence they live here, not in core/cflags.txt -- that file stays the
# single source of the flags that determine numbers.
#
# Without this, editing a .h rebuilt nothing, because the rules named only .c.
# Caught at ROADMAP K4: a field was added to FieldCtx, field.c rebuilt, prop.c
# did not, and two object files disagreed on sizeof the same struct. That did
# not give wrong numbers, it corrupted the heap (malloc(): invalid size) -- the
# loudest possible symptom; the quiet form of the same bug, slightly different
# numbers, would have been incomparably worse.
#
# Same class of hole as ROADMAP D1 describes for watch() in build.rs, with the
# same moral: a check that exists to catch silent changes must itself see all
# of its inputs.
DEPFLAGS := -MMD -MP

# The three libraries are the determinism boundary expressed in the build
# graph:
#
#   libcore.a           core/*.c          RUNTIME, propagation. libm forbidden,
#                                         -lm not linked at all. Second line of
#                                         defence after make check-libm.
#   libcore_offline.a   core/offline/*.c  COOKER. libm allowed, links with -lm.
#                                         Not runtime: computed once on the
#                                         developer's machine, the game ships
#                                         the asset.
#   libcore_planning.a  core/planning/*.c RUNTIME, planning. libm allowed
#                                         (PROJECT.md section 4: the
#                                         determinism boundary runs along
#                                         propagation, not planning).
#                                         scripts/check_no_libm.sh deliberately
#                                         checks only top-level build/core, so
#                                         this subdirectory is outside the libm
#                                         police.
#
# Determinism scenarios link with libcore.a ONLY and without -lm: if
# trigonometry seeps in, the link fails. Tests link with all three.
LDLIBS :=
LDLIBS_OFFLINE := -lm
LDLIBS_PLANNING := -lm

# MinGW appends .exe to executables regardless of -o, so without this make
# would consider the targets unbuilt and rebuild everything every time. MSYS2
# inherits OS=Windows_NT from Windows, so the test is reliable (ROADMAP C5).
EXE :=
ifeq ($(OS),Windows_NT)
EXE := .exe
endif

# Residual momentum removal in the cooker (nbody_anchor_barycentre). On by
# default; turned off only to measure its own effect:
#
#     make cook                        as it ships
#     make cook ANCHOR_BARYCENTRE=0    without removal, for comparison
#
# This is the ONLY thing this variable can pass to the compiler, and the value
# is validated below. There is deliberately no general EXTRA_CFLAGS: it would
# be the hole through which -ffast-math enters the build, and flags must stay
# in core/cflags.txt and nowhere else.
ANCHOR_BARYCENTRE ?= 1
ifeq (,$(filter 0 1,$(ANCHOR_BARYCENTRE)))
$(error ANCHOR_BARYCENTRE must be 0 or 1, not "$(ANCHOR_BARYCENTRE)")
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

# $(sort) matters: scenario order sets the line order in actual.txt, and
# $(wildcard) guarantees no stable order. Without sorting, the golden
# comparison could fail merely because lines were permuted.
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

# The asset is as much an input to the scenarios as their own code:
# sc_ephemeris and sc_trajectory read it at runtime. Without this dependency
# `make cook` changed the asset while the next `make test` compared the OLD
# actual.txt and passed silently -- a check that exists to catch silent changes
# missing the quietest one of all.
FIXTURE  := $(wildcard data/fixture/*.eph)

# Dependency files written by -MMD. Each lists the headers its target depends
# on, in make syntax. `-include` stays quiet when they do not exist yet (first
# build), and -MP adds phony targets for the headers themselves so deleting or
# renaming a .h does not break the build with "no rule to make target".
DEP := $(CORE_OBJ:.o=.d) $(OFFLINE_OBJ:.o=.d) $(PLANNING_OBJ:.o=.d) \
       $(patsubst core/test/%.c,$(BUILD)/test/%.d,$(TEST_SRC)) \
       $(patsubst core/cook/%.c,$(BUILD)/cook/%.d,$(COOK_SRC)) \
       $(patsubst core/export/%.c,$(BUILD)/export/%.d,$(EXPORT_SRC)) \
       $(patsubst core/bench/%.c,$(BUILD)/bench/%.d,$(BENCH_SRC)) \
       $(patsubst core/scenario/%.c,$(BUILD)/scenario/%.d,$(SCEN_SRC))

# Default goal, explicitly, BEFORE the `-include` below. Without it a bare
# `make` built nothing: `-include` brings in rules from the .d files before
# `all:` appears, and the first rule from there (`build/core/accel.o`) became
# the default goal. So `make` quietly said "up to date" having built nothing,
# and the checks that then saw the old library were checking the wrong code.
.DEFAULT_GOAL := all

-include $(DEP)

.PHONY: all test unit asan valgrind coverage check-libm determinism \
        determinism-bless hashes cook cook-dem cook-colour model-ship \
        cook-ship csv plots bench flags clean

all: $(LIB) $(LIB_OFFLINE) $(LIB_PLANNING)

$(BUILD)/core/%.o: core/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -c $< -o $@

$(BUILD)/core/offline/%.o: core/offline/%.c $(ANCHOR_STAMP)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) $(OFFLINE_DEFS) -Icore -Icore/offline -c $< -o $@

# Without this, `make cook ANCHOR_BARYCENTRE=0` after a normal `make` rebuilt
# nothing: make sees files, not variable values. The stamp's name carries the
# value, so changing the value makes it non-existent and every cooker object
# file goes stale. A silent asset cooked by code other than the one asked for
# is exactly the class of bug the rest of this file catches.
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

# Cooker: offline, libm allowed.
$(BUILD)/cook/%$(EXE): core/cook/%.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/offline -o $@ $< \
		$(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# CSV exporters. Linked like tests, with both libraries and -lm: these are
# diagnostic drivers, not runtime, and one of them (ex_horizons) deliberately
# runs the offline mutual N-body against Horizons. The live "no libm at
# runtime" check comes from the scenarios below; duplicating it here adds
# nothing.
$(BUILD)/export/%$(EXE): core/export/%.c core/export/csv.c $(LIB) $(LIB_OFFLINE)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -Icore/offline -Icore/export -o $@ \
		$< core/export/csv.c $(LIB_OFFLINE) $(LIB) $(LDLIBS_OFFLINE)

# Without libcore_offline.a and without -lm: linking here is the live check
# that the runtime part holds no libm.
$(BUILD)/scenario/%$(EXE): core/scenario/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

# The same runtime libcore.a, without -lm: the benchmark claims to measure the
# throughput of the detailed physics (CLAUDE.md, invariant 3 -- no libm in the
# integration loop), and linking without -lm is the live check of that rather
# than optimism.
$(BUILD)/bench/%$(EXE): core/bench/%.c $(LIB)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(DEPFLAGS) -Icore -o $@ $< $(LIB) $(LDLIBS)

# --- Checks ----------------------------------------------------------------

# The order is deliberate: cheapest and most specific check first.
test: check-libm unit determinism
	@echo ""
	@echo "ALL CHECKS PASSED"

check-libm: $(LIB)
	@sh scripts/check_no_libm.sh $(BUILD)/core

unit: $(TEST_BIN)
	@fail=0; \
	for t in $(TEST_BIN); do \
		echo "== $$t"; \
		$$t || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "UNIT TESTS FAILED"; exit 1; fi

# The same unit tests built with ASan+UBSan. Not part of `make test` and not
# part of the determinism gate: this checks MEMORY, not numbers.
#
# Why separate when CI has valgrind: that step runs only over the core-sys and
# core-rs test binaries (.github/workflows/valgrind.yml explains why). Nobody
# checked the C unit tests, and it showed exactly as it should: in test_prop,
# the K6b block propagated through an already freed context, glibc handed the
# freed memory back as if nothing happened, both Linuxes were green -- and only
# macOS fell over, with a segfault two steps from the cause. Here it shows as
# heap-use-after-free with both stacks.
#
# Three things that are easy to get wrong:
#
#   1. -fno-sanitize-recover=all is MANDATORY. By default UBSan prints
#      diagnostics and CARRIES ON, leaving the exit code zero. Without this
#      flag the check would go green having printed an error -- worse than not
#      existing.
#   2. A separate $(ASAN_DIR) tree. The flags differ here while the object file
#      names would be the same, so mixing them with the normal build would mean
#      linking numbers built by who knows what. Hence no .o here but a direct
#      pass from .c to binary: rebuilt every time, but nothing to confuse.
#   3. -lm is always linked. The "libm police" does not apply to this target:
#      everything is built into one binary here, and the live link-time check
#      comes from the scenarios, not from this target.
ASAN_DIR := $(BUILD)/asan
ASAN_BIN := $(patsubst core/test/%.c,$(ASAN_DIR)/%$(EXE),$(TEST_SRC))
ASAN_SRC := $(CORE_SRC) $(OFFLINE_SRC) $(PLANNING_SRC)
ASAN_FLAGS := -fsanitize=address,undefined -fno-sanitize-recover=all \
              -fno-omit-frame-pointer

$(ASAN_DIR)/%$(EXE): core/test/%.c $(ASAN_SRC)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(ASAN_FLAGS) $(OFFLINE_DEFS) \
		-Icore -Icore/offline -Icore/planning -o $@ $< $(ASAN_SRC) \
		$(LDLIBS_OFFLINE)

# stdout to /dev/null deliberately: what the tests compute was already said by
# `make unit`, and what matters here is only the sanitizers staying silent.
# Both write to stderr, so an error report is visible while hundreds of lines
# of diagnostics are not.
asan: $(ASAN_BIN)
	@fail=0; \
	for t in $(ASAN_BIN); do \
		echo "== $$t"; \
		$$t > /dev/null || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "ASAN/UBSAN FAILED"; exit 1; fi; \
	echo ""; \
	echo "asan: no memory errors"

# The same unit tests under valgrind. NOT part of `make test`: it checks
# memory, not numbers, and it is slow (about six minutes against seconds).
#
# Why SEPARATE from `make asan` when both are about memory: ASan does not see
# reads of uninitialised memory at all. That was exactly bug K7b -- `test_target`
# built a FieldCtx by hand and left most of the struct as whatever was on the
# stack. ASan was green locally, so was `make test`, and it fell over only on
# windows-mingw in CI, three steps from the cause. Valgrind shows it on the
# first line, with function name and line number.
#
# --leak-check=no deliberately: the unit tests do not free everything they
# allocate, and making that red would train us to ignore red (the same argument
# the workflow uses for why valgrind is not run over the engine). The target
# here is uninitialised reads and invalid accesses.
VALGRIND ?= valgrind

valgrind: $(TEST_BIN)
	@fail=0; \
	for t in $(TEST_BIN); do \
		echo "== $$t"; \
		$(VALGRIND) --quiet --leak-check=no --error-exitcode=9 $$t \
			> /dev/null || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "VALGRIND FAILED"; exit 1; fi; \
	echo ""; \
	echo "valgrind: no uninitialised reads and no memory errors"

# Line coverage of the C sources by the unit tests -- the README badge.
#
# NOT a gate, and deliberately not part of `make test`. A percentage says where
# tests are missing; it does not say whether the code is right. Turned into a
# threshold it buys tests written to move the number.
#
# Third target with flags of its own, after asan and valgrind, for the same
# reason as those two: --coverage changes codegen, so it must never touch the
# tree the numbers come from. core/cflags.txt stays the only source of the
# flags that determine numbers -- these determine nothing but counters.
#
# The one thing here that is easy to get wrong: the objects are compiled ONCE
# into $(COV_DIR) and every test binary links THE SAME object files. That is
# what makes the counters merge -- gcc writes one .gcda per object file and
# each run adds to it, so a line covered by any test counts once. Compiling the
# sources into every binary separately, the way `make asan` does, would instead
# give one set of counters per binary (gcc names the aux files after the
# output), and 31 partial reports cannot be summed: a line reached by two tests
# would count twice.
COV_DIR   := $(BUILD)/cov
COV_SRC   := $(CORE_SRC) $(OFFLINE_SRC) $(PLANNING_SRC)
COV_OBJ   := $(patsubst %.c,$(COV_DIR)/%.o,$(COV_SRC))
COV_BIN   := $(patsubst core/test/%.c,$(COV_DIR)/test/%$(EXE),$(TEST_SRC))
COV_FLAGS := --coverage

# Without this make treats the instrumented objects as intermediate files --
# built by one pattern rule, used only by another -- and DELETES them after
# every run, printing thirty rm lines and recompiling the whole core next time.
.SECONDARY: $(COV_OBJ)

$(COV_DIR)/%.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(COV_FLAGS) $(OFFLINE_DEFS) \
		-Icore -Icore/offline -Icore/planning -c $< -o $@

# -lm always, as in the asan target: the "libm police" checks the shipped
# build/core tree, and this is not it.
$(COV_DIR)/test/%$(EXE): core/test/%.c $(COV_OBJ)
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(COV_FLAGS) -Icore -Icore/offline -Icore/planning \
		-o $@ $< $(COV_OBJ) $(LDLIBS_OFFLINE)

# Stale .gcda are deleted before the run: counters ACCUMULATE across runs by
# design, so without this every `make coverage` would add the previous run to
# itself. The percentage would barely move -- which is exactly why the mistake
# would survive unnoticed.
#
# stdout to /dev/null, like `make asan`: what the tests compute was already
# said by `make unit`, and what matters here is only that they all ran.
coverage: $(COV_BIN)
	@find $(COV_DIR) -name '*.gcda' -delete
	@fail=0; \
	for t in $(COV_BIN); do \
		echo "== $$t"; \
		$$t > /dev/null || fail=1; \
	done; \
	if [ $$fail -ne 0 ]; then echo "COVERAGE RUN FAILED"; exit 1; fi
	@sh scripts/coverage_c.sh $(COV_DIR) $(COV_SRC)

$(ACTUAL): $(SCEN_BIN) $(FIXTURE)
	@mkdir -p $(dir $@)
	@rm -f $@
	@for s in $(SCEN_BIN); do $$s >> $@; done

determinism: $(ACTUAL)
	@if [ ! -f $(GOLDEN) ]; then \
		echo "determinism: no golden file -- run make determinism-bless" >&2; \
		exit 1; \
	fi
	@if diff -u $(GOLDEN) $(ACTUAL) > $(BUILD)/scenario/diff.txt; then \
		echo "determinism: hashes match the golden file"; \
	else \
		echo "determinism: FAILED -- hashes diverged from $(GOLDEN)" >&2; \
		cat $(BUILD)/scenario/diff.txt >&2; \
		echo "" >&2; \
		echo "  If the behaviour change is intended: make determinism-bless" >&2; \
		echo "  and show the diff in the commit. If not, this is a regression." >&2; \
		exit 1; \
	fi

determinism-bless: $(ACTUAL)
	@cp $(ACTUAL) $(GOLDEN)
	@echo "Golden hashes updated. Review git diff $(GOLDEN) before committing:"
	@echo "  a change here means the simulation result changed."

# For comparing by hand across machines, when there is no CI yet or when it
# has already failed and the diff needs eyes (ROADMAP C5).
hashes: $(ACTUAL)
	@cat $(ACTUAL)

# The asset fixture is committed deliberately. The cooker uses cos() via
# Chebyshev fitting, so two computers would compute DIFFERENT coefficients and
# the cross-platform comparison would fail on the cooker rather than on the
# runtime it is meant to check. Cook once, commit, compare (ROADMAP C5).
cook: $(COOK_BIN)
	@mkdir -p data/fixture
	@$(BUILD)/cook/cook_fixture$(EXE)
	@echo ""
	@echo "Regenerated. Check git diff and what drives the change:"
	@echo "  a change here changes every scenario hash that reads the asset."

# --- Terrain cooker (ROADMAP-PLANETS.md, R5b) ------------------------------
#
# A separate target from `cook`, and not out of pedantry: that one regenerates
# the asset fixture in git and changes every scenario hash, while this one
# writes to `/assets/`, which is not in git at all. Confusing them is expensive
# in exactly one direction.
.PHONY: cook-dem
cook-dem:
	cargo run --release -p dem-cook

# Moon colour from the LROC WAC mosaic (stage T, T2d).
#
# A separate target from `cook-dem`: different source, different pyramid depth
# (6 against 5), and it can be missing independently -- raw content data lives
# outside git (Q5), and `data/wac/README.md` says how to put the mosaic on
# disk.
.PHONY: cook-colour
cook-colour:
	cargo run --release -p dem-cook -- --colour

# The star catalogue from Yale BSC5 (stage Z, Z2).
#
# Its own target for the same reason as `cook-colour`: a different source that
# can be missing on its own. The catalogue is outside git (Q5) and putting it
# on disk is still manual -- debt D18 -- so `star-cook` prints the two commands
# that fetch it when the file is not there.
.PHONY: cook-stars
cook-stars:
	cargo run --release -p star-cook

# Ship from Blender (stage T, T5d).
#
# Two targets, because the tools differ and are needed at different times.
# `model-ship` calls Blender and rewrites `assets-src/` -- files in git; do it
# deliberately and read the diff. `cook-ship` only translates the already
# committed export into `/assets/`, which is not in git, and needs no Blender
# at all.
#
# Blender's path is a variable: on this machine it is installed through Steam
# and is not on `PATH` (skill `blender-assets`). Set your own with:
#   make model-ship BLENDER=/path/to/blender
BLENDER ?= $(HOME)/snap/steam/common/.steam/steam/steamapps/common/Blender/blender

.PHONY: model-ship
model-ship:
	$(BLENDER) -b --factory-startup -noaudio -P tools/blender/ship.py -- assets-src
	@echo ""
	@echo "Regenerated assets-src/. Check git diff: the .gltf carries the"
	@echo "Blender version, so a diff can appear with no model change."

.PHONY: cook-ship
cook-ship:
	cargo run --release -p mesh-cook

# --- M0 deliverable: look with your eyes -----------------------------------
#
# Tests say "passed"; this shows what was actually computed. Both are needed: a
# trajectory that stays inside tolerance while flying the wrong way is a thing
# that happens, and the eye catches it, not a threshold.
#
# CSV are build artefacts, not in git (.gitignore). They are NOT part of the
# determinism comparison: printing a double as decimal text is libc's business,
# not IEEE's, so these files cannot be compared across platforms. The scenario
# hashes exist for that (core/export/csv.h).
csv: $(EXPORT_BIN)
	@mkdir -p $(CSV_DIR)
	@for e in $(EXPORT_BIN); do echo "== $$e"; $$e || exit 1; done
	@echo ""
	@echo "CSV in $(CSV_DIR). Next: make plots"

# matplotlib is deliberately not a build dependency: the core does not use it
# and CI does not have it. The script itself says what is missing.
plots: csv
	@mkdir -p $(PLOT_DIR)
	@$(PYTHON) scripts/plot.py --csv $(CSV_DIR) --out $(PLOT_DIR)

# DOP853 throughput on this machine, as a number, not a hash (skill
# perf-probe). Wall-clock time is unstable across runs and machines by nature,
# which is exactly why the result is not part of determinism.
bench: $(BENCH_BIN)
	@for b in $(BENCH_BIN); do echo "== $$b"; $$b; done

flags:
	@echo $(CFLAGS)

clean:
	rm -rf $(BUILD)
