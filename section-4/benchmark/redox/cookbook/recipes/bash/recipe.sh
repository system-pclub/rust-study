VERSION=4.4
TAR=http://ftp.gnu.org/gnu/bash/bash-$VERSION.tar.gz
BUILD_DEPENDS=(gettext)

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
    export LDFLAGS="-L$sysroot/lib -static"
    export CPPFLAGS="-I$sysroot/include"
    wget -O support/config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub
    ./configure \
        --build=${BUILD} \
        --host=${HOST} \
        --prefix=/ \
        --disable-readline \
        bash_cv_getenv_redef=no
    make # -j"$(nproc)"
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
    make DESTDIR="$dest" ${MAKEFLAGS} install
    skip=1
}
