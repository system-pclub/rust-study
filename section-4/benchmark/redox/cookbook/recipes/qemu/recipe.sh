VERSION=3.1.0
TAR=https://download.qemu.org/qemu-$VERSION.tar.xz
BUILD_DEPENDS=(curl glib libiconv libpng pcre pixman sdl zlib)

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
    export CFLAGS="-I$sysroot/include"
    export CPPFLAGS="-I$sysroot/include"
    export LDFLAGS="-L$sysroot/lib"
    ./configure \
        --build=${BUILD} \
        --host="${HOST}" \
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
    #export LLVM_CONFIG="x86_64-unknown-redox-llvm-config"
    dest="$(realpath $1)"
    make DESTDIR="$dest" install
    rm -f "$dest/lib/"*.la
    skip=1
}
