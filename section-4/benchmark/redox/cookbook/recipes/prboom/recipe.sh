VERSION=2.5.0
TAR=https://downloads.sourceforge.net/project/prboom/prboom%20stable/$VERSION/prboom-$VERSION.tar.gz
BUILD_DEPENDS=(sdl liborbital)

function recipe_version {
    echo "$VERSION"
    skip=1
}

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_build {
    export CFLAGS="-static"
    sysroot="$(realpath ../sysroot)"
    autoreconf -if
    wget -O autotools/config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub
    ./configure \
        --prefix=/ \
        --build=${BUILD} \
        --host=${HOST} \
        --disable-sdltest \
        --disable-cpu-opt \
        --disable-gl \
        --without-net \
        --with-sdl-prefix="$sysroot"
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
    skip=1
}
