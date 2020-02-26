VERSION=1.14.6
GIT=https://github.com/wesnoth/wesnoth.git
BRANCH=$VERSION
BUILD_DEPENDS=(
    cairo
    freetype
    glib
    libjpeg
    liborbital
    libpng
    libvorbis
    llvm
    mesa mesa_glu
    pcre
    pixman
    sdl2 sdl2_image sdl2_mixer sdl2_ttf
    zlib
)

function recipe_version {
    printf "$VERSION"
    skip=1
}

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_build {
    sysroot="$(realpath ../sysroot)"
    export CFLAGS="-I$sysroot/include"
    export LDFLAGS="-L$sysroot/lib"

    rm -rf build
    mkdir -p build
    cd build
    set -x
    cmake \
        -DCMAKE_INSTALL_PREFIX:PATH=/ \
        -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
        -DENABLE_SERVER=OFF \
        -DENABLE_TESTS=OFF \
        -DCRYPTO_LIBRARY=openssl \
        -DSDL2_LIBRARY=sdl2 \
        -DSDL2_IMAGE_LIBRARY=SDL2_image \
        -DSDL2_MIXER_LIBRARY=SDL2_mixer \
        -DSDL2_TTF_LIBRARY=SDL2_ttf \
        -DVORBISFILE_INCLUDE_DIR="${sysroot}/include" \
        -DVORBISFILE_LIBRARY=vorbisfile \
        ..
    VERBOSE=1 make all -j"$(nproc)"
    set +x
    skip=1
}

function recipe_test {
    echo "skipping test"
    skip=1
}

function recipe_clean {
    rm -rf build
    skip=1
}

function recipe_stage {
    dest="$(realpath $1)"
    mkdir -pv "$dest/bin"
    cp "build/wesnoth" "$dest/bin/wesnoth"
    skip=1
}
