BUILD_DEPENDS=(cairo expat fontconfig freetype liborbital libpng pixman zlib)

function recipe_version {
    printf "1.0.0"
    skip=1
}

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_prepare {
    rm -rf source
    mkdir source
    cp cairodemo.c source
}

function recipe_build {
    sysroot="$(realpath ../sysroot)"
    export LDFLAGS="-L$sysroot/lib"
    export CPPFLAGS="-I$sysroot/include"
    set -x
    "${CXX}" $("${PKG_CONFIG}" --cflags cairo) cairodemo.c -o cairodemo -static $("${PKG_CONFIG}" --libs cairo) -lorbital
    set +x
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
    mkdir -pv "$dest/bin"
    cp -v "cairodemo" "$dest/bin/cairodemo"
    skip=1
}
