VERSION=3.3
TAR=https://sourceforge.net/projects/vice-emu/files/releases/vice-$VERSION.tar.gz/download
TAR_SHA256=1a55b38cc988165b077808c07c52a779d181270b28c14b5c9abf4e569137431d
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
    sysroot="$(realpath ../sysroot)"
    wget -O config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub

    export sdl_config="$sysroot/bin/sdl-config"
    export CFLAGS="-I$sysroot/include -I$sysroot/include/SDL"
    export CXXFLAGS="$CFLAGS"
    export LDFLAGS="-L$sysroot/lib -static"

    ./configure \
        --build=${BUILD} \
        --host=${HOST} \
        --prefix='' \
        --enable-sdlui \
        --disable-sdlui2 \
        --disable-rs232 \
        --disable-realdevice \
        --disable-midi
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
