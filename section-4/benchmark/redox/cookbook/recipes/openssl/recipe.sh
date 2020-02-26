GIT=https://gitlab.redox-os.org/redox-os/openssl.git
BRANCH=redox
GIT_UPSTREAM=https://github.com/openssl/openssl.git

function recipe_version {
    printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
    skip=1
}

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_build {
    ./Configure no-shared no-dgram redox-$ARCH --prefix="/"
    make -j"$(nproc)"
    skip=1
}

function recipe_test {
    echo "skipping test"
    skip=1
}

function recipe_clean {
    make clean
    skip=1
}

function recipe_stage {
    dest="$(realpath $1)"
    make DESTDIR="$dest" install
    rm -rf "$1/{share,ssl}"
    skip=1
}
