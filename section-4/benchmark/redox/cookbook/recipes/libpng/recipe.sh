VERSION=1.6.36
TAR=https://github.com/glennrp/libpng/archive/v${VERSION}.tar.gz
BUILD_DEPENDS=(zlib)

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
    export CPPFLAGS="-I$sysroot/include"
    export LDFLAGS="-L$sysroot/lib --static"
    chmod +w config.sub
    wget -O config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub
    ./configure --build=${BUILD} --host=${HOST} --prefix='/'
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
    rm -f "$dest/bin/"*-config "$dest/lib/"*.la
    skip=1
}
