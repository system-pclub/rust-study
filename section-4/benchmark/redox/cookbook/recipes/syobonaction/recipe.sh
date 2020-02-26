VERSION=1.0-rc3
GIT=https://github.com/angelXwind/OpenSyobonAction
BUILD_DEPENDS=(sdl liborbital sdl_mixer sdl_image sdl_gfx sdl_ttf freetype libjpeg libpng zlib libogg libvorbis)

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
    export SDL_CONFIG="${PKG_CONFIG} sdl"
    export CPPFLAGS="-I$sysroot/include"
    export LDFLAGS="-L$sysroot/lib"
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
    mkdir -pv "$1/bin"
    mkdir -pv "$1/share/syobonaction"
    cp -Rv ./SyobonAction "$1/bin/syobonaction"
    cp -Rv ./BGM "$1/share/syobonaction"
    cp -Rv ./res "$1/share/syobonaction"
    cp -Rv ./SE "$1/share/syobonaction"
    skip=1
}
