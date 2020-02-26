VERSION=1.13
TAR=https://dev.yorhel.nl/download/ncdu-$VERSION.tar.gz
BUILD_DEPENDS=(ncurses)
DEPENDS=(terminfo)

function recipe_version {
    echo "$VERSION"
    skip=1
}
function recipe_update {
    echo "skipping update"
    skip=1
}
function recipe_build {
    sysroot="$PWD/../sysroot"
    export CPPFLAGS="-I$sysroot/include -I$sysroot/include/ncurses"
    export LDFLAGS="-L$sysroot/lib -static"
    ./configure \
        --build=${BUILD} \
        --host="$HOST" \
        --prefix=/
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
    dest="$(realpath "$1")"
    make DESTDIR="$dest" install
    skip=1
}
