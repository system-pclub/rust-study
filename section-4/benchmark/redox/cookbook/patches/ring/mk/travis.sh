#!/usr/bin/env bash
#
# Copyright 2015 Brian Smith.
#
# Permission to use, copy, modify, and/or distribute this software for any
# purpose with or without fee is hereby granted, provided that the above
# copyright notice and this permission notice appear in all copies.
#
# THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
# WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
# MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
# SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
# WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
# OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
# CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

set -eux -o pipefail
IFS=$'\n\t'

printenv

case $TARGET_X in
aarch64-unknown-linux-gnu)
  export QEMU_LD_PREFIX=/usr/aarch64-linux-gnu
  ;;
arm-unknown-linux-gnueabihf)
  export QEMU_LD_PREFIX=/usr/arm-linux-gnueabihf
  ;;
armv7-linux-androideabi)
  # install the android sdk/ndk
  mk/travis-install-android.sh

  export PATH=$HOME/android/android-18-arm-linux-androideabi-4.8/bin:$PATH
  export PATH=$HOME/android/android-sdk-linux/platform-tools:$PATH
  export PATH=$HOME/android/android-sdk-linux/tools:$PATH
  ;;
*)
  ;;
esac

if [[ "$TARGET_X" =~ ^(arm|aarch64) && ! "$TARGET_X" =~ android ]]; then
  # We need a newer QEMU than Travis has.
  # sudo is needed until the PPA and its packages are whitelisted.
  # See https://github.com/travis-ci/apt-source-whitelist/issues/271
  sudo add-apt-repository ppa:pietro-monteiro/qemu-backport -y
  sudo apt-get update -qq
  sudo apt-get install --no-install-recommends binfmt-support qemu-user-binfmt -y
fi

if [[ ! "$TARGET_X" =~ "x86_64-" ]]; then
  rustup target add "$TARGET_X"

  # By default cargo/rustc seems to use cc for linking, We installed the
  # multilib support that corresponds to $CC_X but unless cc happens to match
  # $CC_X, that's not the right version. The symptom is a linker error
  # where it fails to find -lgcc_s.
  if [[ ! -z "${CC_X-}" ]]; then
    mkdir .cargo
    echo "[target.$TARGET_X]" > .cargo/config
    echo "linker= \"$CC_X\"" >> .cargo/config
    cat .cargo/config
  fi
fi

if [[ ! -z "${CC_X-}" ]]; then
  export CC=$CC_X
  $CC --version
else
  cc --version
fi

cargo version
rustc --version

if [[ "$MODE_X" == "RELWITHDEBINFO" ]]; then
  mode=--release
  target_dir=target/$TARGET_X/release
else
  target_dir=target/$TARGET_X/debug
fi

case $TARGET_X in
armv7-linux-androideabi)
  cargo test -vv -j2 --no-run ${mode-} ${FEATURES_X-} --target=$TARGET_X
  # TODO: There used to be some logic for running the tests here using the
  # Android emulator. That was removed because something broke this. See
  # https://github.com/briansmith/ring/issues/603.
  ;;
*)
  cargo test -vv -j2 ${mode-} ${FEATURES_X-} --target=$TARGET_X
  ;;
esac

if [[ "$KCOV" == "1" ]]; then
  # kcov reports coverage as a percentage of code *linked into the executable*
  # (more accurately, code that has debug info linked into the executable), not
  # as a percentage of source code. Thus, any code that gets discarded by the
  # linker due to lack of usage isn't counted at all. Thus, we have to re-link
  # with "-C link-dead-code" to get accurate code coverage reports.
  # Alternatively, we could link pass "-C link-dead-code" in the "cargo test"
  # step above, but then "cargo test" we wouldn't be testing the configuration
  # we expect people to use in production.
  cargo clean
  RUSTFLAGS="-C link-dead-code" \
    cargo test -vv --no-run -j2  ${mode-} ${FEATURES_X-} --target=$TARGET_X
  mk/travis-install-kcov.sh
  for test_exe in `find target/$TARGET_X/debug -maxdepth 1 -executable -type f`; do
    ${HOME}/kcov-${TARGET_X}/bin/kcov \
      --verify \
      --coveralls-id=$TRAVIS_JOB_ID \
      --exclude-path=/usr/include \
      --include-pattern="ring/crypto,ring/src,ring/tests" \
      target/kcov \
      $test_exe
  done
fi

# Verify that `cargo build`, independent from `cargo test`, works; i.e. verify
# that non-test builds aren't trying to use test-only features. For platforms
# for which we don't run tests, this is the only place we even verify that the
# code builds.
cargo build -vv -j2 ${mode-} ${FEATURES_X-} --target=$TARGET_X

echo end of mk/travis.sh
