#!/usr/bin/env bash
set -ex

if [[ -z "$INTEGRATION" ]]; then
    exit 0
fi

CARGO_TARGET_DIR=$(pwd)/target/
export CARGO_TARGET_DIR

rm ~/.cargo/bin/cargo-clippy
cargo install --force --debug --path .

echo "Running integration test for crate ${INTEGRATION}"

git clone --depth=1 "https://github.com/${INTEGRATION}.git" checkout
cd checkout

# run clippy on a project, try to be verbose and trigger as many warnings
# as possible for greater coverage
# NOTE: we use `tee` to print any warnings and errors to stdout
#       to avoid build timeout in Travis
RUST_BACKTRACE=full \
cargo clippy \
    --all-targets \
    --all-features \
    -- \
    --cap-lints warn \
    -W clippy::pedantic \
    -W clippy::nursery \
    2>&1 | tee clippy_output

cargo uninstall clippy

if grep -q "internal compiler error\|query stack during panic\|E0463" clippy_output; then
    exit 1
fi
