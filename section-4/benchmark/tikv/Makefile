SHELL := /bin/bash
ENABLE_FEATURES ?= default

# Disable portable on MacOS to sidestep the compiler bug in clang 4.9
ifeq ($(shell uname -s),Darwin)
ROCKSDB_SYS_PORTABLE=0
endif

# Build portable binary by default unless disable explicitly
ifneq ($(ROCKSDB_SYS_PORTABLE),0)
ENABLE_FEATURES += portable
endif

# Enable sse4.2 by default unless disable explicitly
ifneq ($(ROCKSDB_SYS_SSE),0)
ENABLE_FEATURES += sse
endif

ifneq ($(FAIL_POINT),1)
ENABLE_FEATURES += no-fail
endif

PROJECT_DIR:=$(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))

DEPS_PATH = $(CURDIR)/tmp
BIN_PATH = $(CURDIR)/bin
GOROOT ?= $(DEPS_PATH)/go
CARGO_TARGET_DIR ?= $(CURDIR)/target

BUILD_INFO_GIT_FALLBACK := "Unknown (no git or not git repo)"
BUILD_INFO_RUSTC_FALLBACK := "Unknown"
export TIKV_BUILD_TIME := $(shell date -u '+%Y-%m-%d %I:%M:%S')
export TIKV_BUILD_GIT_HASH := $(shell git rev-parse HEAD 2> /dev/null || echo ${BUILD_INFO_GIT_FALLBACK})
export TIKV_BUILD_GIT_BRANCH := $(shell git rev-parse --abbrev-ref HEAD 2> /dev/null || echo ${BUILD_INFO_GIT_FALLBACK})
export TIKV_BUILD_RUSTC_VERSION := $(shell rustc --version 2> /dev/null || echo ${BUILD_INFO_RUSTC_FALLBACK})
LATEST_AUDIT_VERSION = $(strip $(shell cargo search cargo-audit | head -n 1 | awk '{ gsub(/"/, "", $$3); print $$3 }'))
CURRENT_AUDIT_VERSION = $(strip $(shell (cargo audit --version 2> /dev/null || echo "noop 0") | awk '{ print $$2 }'))

default: release

.PHONY: all

all: format build test

pre-clippy: unset-override
	@rustup component add clippy

clippy: pre-clippy
	@cargo clippy --all --all-targets -- \
		-A clippy::module_inception -A clippy::needless_pass_by_value -A clippy::cyclomatic_complexity \
		-A clippy::unreadable_literal -A clippy::should_implement_trait -A clippy::verbose_bit_mask \
		-A clippy::implicit_hasher -A clippy::large_enum_variant -A clippy::new_without_default \
		-A clippy::neg_cmp_op_on_partial_ord -A clippy::too_many_arguments \
		-A clippy::excessive_precision -A clippy::collapsible_if -A clippy::blacklisted_name \
		-A clippy::needless_range_loop -D rust-2018-idioms -A clippy::redundant_closure \
		-A clippy::match_wild_err_arm -A clippy::blacklisted_name

dev: format clippy
	@env FAIL_POINT=1 make test

build:
	cargo build --features "${ENABLE_FEATURES}"

ctl:
	cargo build --release --features "${ENABLE_FEATURES}" --bin tikv-ctl
	@mkdir -p ${BIN_PATH}
	@cp -f ${CARGO_TARGET_DIR}/release/tikv-ctl ${BIN_PATH}/

run:
	cargo run --features "${ENABLE_FEATURES}" --bin tikv-server

release:
	cargo build --release --features "${ENABLE_FEATURES}"
	@mkdir -p ${BIN_PATH}
	@cp -f ${CARGO_TARGET_DIR}/release/tikv-ctl ${CARGO_TARGET_DIR}/release/tikv-server ${CARGO_TARGET_DIR}/release/tikv-importer ${BIN_PATH}/
	bash scripts/check-sse4_2.sh

unportable_release:
	ROCKSDB_SYS_PORTABLE=0 make release

prof_release:
	ENABLE_FEATURES=mem-profiling make release

fail_release:
	FAIL_POINT=1 make release

# unlike test, this target will trace tests and output logs when fail test is detected.
trace_test:
	env CI=true SKIP_FORMAT_CHECK=true FAIL_POINT=1 ${PROJECT_DIR}/ci-build/test.sh

test:
        # When SIP is enabled, DYLD_LIBRARY_PATH will not work in subshell, so we have to set it
        # again here. LOCAL_DIR is defined in .travis.yml.
        # The special linux case below is testing the mem-profiling
        # features in tikv_alloc, which are marked #[ignore] since
        # they require special compile-time and run-time setup
        # Forturately rebuilding with the mem-profiling feature will only
        # rebuild starting at jemalloc-sys.
	export DYLD_LIBRARY_PATH="${DYLD_LIBRARY_PATH}:${LOCAL_DIR}/lib" && \
	export LOG_LEVEL=DEBUG && \
	export RUST_BACKTRACE=1 && \
	cargo test --features "${ENABLE_FEATURES}" --all ${EXTRA_CARGO_ARGS} -- --nocapture && \
	cargo test --features "${ENABLE_FEATURES}" --bench misc ${EXTRA_CARGO_ARGS} -- --nocapture  && \
	if [[ "`uname`" == "Linux" ]]; then \
		export MALLOC_CONF=prof:true,prof_active:false && \
		cargo test --features "${ENABLE_FEATURES},mem-profiling" ${EXTRA_CARGO_ARGS} --bin tikv-server -- --nocapture --ignored; \
	fi
	bash scripts/check-bins-for-jemalloc.sh

bench:
	LOG_LEVEL=ERROR RUST_BACKTRACE=1 cargo bench --all --features "${ENABLE_FEATURES}" -- --nocapture

unset-override:
	@# unset first in case of any previous overrides
	@if rustup override list | grep `pwd` > /dev/null; then rustup override unset; fi

pre-format: unset-override
	@rustup component add rustfmt

format: pre-format
	@cargo fmt --all -- --check >/dev/null || \
	cargo fmt --all

pre-audit:
ifneq ($(LATEST_AUDIT_VERSION),$(CURRENT_AUDIT_VERSION))
	cargo install cargo-audit --force
endif

audit: pre-audit
	cargo audit

clean:
	cargo clean

expression: format clippy
	LOG_LEVEL=ERROR RUST_BACKTRACE=1 cargo test --features "${ENABLE_FEATURES}" "coprocessor::dag::expr" -- --nocapture



# The below x- targets are temporary, for experimenting with new profiles,
# specifically in pursuit of compile time speedups.
#
# re https://github.com/tikv/tikv/issues/4189
#
# The idea here is that there are more "profiles" than just "dev" and "release".
# In particular, there is an optimized dev profile, here "dev-opt". The below
# profiles are intentionally named differently from the stock cargo 'dev'
# and 'release' profiles to avoid confusion, but eventually we might expect
# e.g. dev-opt to become the 'release' and 'bench' profiles, and 'dev-opt'
# to become the 'dev' and perhaps 'test' profiles; with the _real_ release
# profile being created with a config file.
#
# They can be invoked as:
#
#     $ make x-build-dev-nopt      # An unoptimized build
#                                  #   (fast build / slow run)
#     $ make x-build-dev-opt       # A mostly-optimized dev profile
#                                  #   (slower build / faster run)
#     $ make x-build-prod          # A release build
#                                  #   (slowest build / fastest run)
#     $ make x-bench               # Run benches mostly-optimized
#                                  #   (slower build / faster run)
#     $ make x-test                # Run tests unoptimized
#                                  #   (fast build / slow run)
#
# The first three have aliases:
#
#     $ make x-build
#     $ make x-build-opt
#     $ make x-release
#
# The below rules all rely on using a .cargo/config file to override various
# profiles. Within those config files we'll experiment with compile-time
# optimizations which can't be done with Cargo.toml alone.
#
# Eventually, we'll merge as much of the configs into Cargo.toml as possible,
# and merge the below commands into the rest of the makefile.
#
# None of the build profiles has debuginfo on by default because it increases
# the build time by ~20%. The easiest way to build with debuginfo is by setting
# the DEBUG makefile variable,
#
#     $ make x-build DEBUG=1
#
# To pass extra arguments to cargo you can set CARGO_ARGS,
#
#     $ make x-build CARGO_ARGS="--all"

DEV_OPT_CONFIG=etc/cargo.config.dev-opt
DEV_NOPT_CONFIG=etc/cargo.config.dev-nopt
PROD_CONFIG=etc/cargo.config.prod
TEST_CONFIG=etc/cargo.config.test
BENCH_CONFIG=etc/cargo.config.bench

ifneq ($(DEBUG),)
export X_DEBUG=${DEBUG}
endif

export X_CARGO_ARGS:=${CARGO_ARGS}

x-build-dev-opt: export X_CARGO_CMD=build
x-build-dev-opt: export X_CARGO_FEATURES=${ENABLE_FEATURES}
x-build-dev-opt: export X_CARGO_RELEASE=1
x-build-dev-opt: export X_CARGO_CONFIG_FILE=${DEV_OPT_CONFIG}
x-build-dev-opt:
	bash scripts/run-cargo.sh

x-build-dev-nopt: export X_CARGO_CMD=build
x-build-dev-nopt: export X_CARGO_FEATURES=${ENABLE_FEATURES}
x-build-dev-nopt: export X_CARGO_RELEASE=0
x-build-dev-nopt: export X_CARGO_CONFIG_FILE=${DEV_NOPT_CONFIG}
x-build-dev-nopt:
	bash scripts/run-cargo.sh

# This is a profile for _actual releases_. Devs should almost never be using
# this. It is a very slow build, and only a bit faster.
x-build-prod: export X_CARGO_CMD=build
x-build-prod: export X_CARGO_FEATURES=${ENABLE_FEATURES}
x-build-prod: export X_CARGO_RELEASE=1
x-build-prod: export X_CARGO_CONFIG_FILE=${PROD_CONFIG}
x-build-prod:
	bash scripts/run-cargo.sh

x-test: export X_CARGO_CMD=test
x-test: export X_CARGO_FEATURES=${ENABLE_FEATURES}
x-test: export X_CARGO_RELEASE=0
x-test: export X_CARGO_CONFIG_FILE=${TEST_CONFIG}
x-test:
	bash etc/run-cargo.sh

x-bench: export X_CARGO_CMD=bench
x-bench: export X_CARGO_FEATURES=${ENABLE_FEATURES}
x-bench: export X_CARGO_RELEASE=0
x-bench: export X_CARGO_CONFIG_FILE=${BENCH_CONFIG}
x-bench:
	bash etc/run-cargo.sh

x-build: x-build-dev-nopt

x-build-opt: x-build-dev-opt

x-release: x-build-prod

# Devs might want to use the config files but not the makefiles.
# These are rules to put each config file in place.

x-dev-opt-config:
	mkdir -p .cargo && cp -b "${DEV_OPT_CONFIG}" .cargo/config

x-dev-nopt-config:
	mkdir -p .cargo && cp -b "${DEV_NOPT_CONFIG}" .cargo/config

x-prod-config:
	mkdir -p .cargo && cp -b "${PROD_CONFIG}" .cargo/config

x-clean:
	-rm -r .cargo
	cargo clean
