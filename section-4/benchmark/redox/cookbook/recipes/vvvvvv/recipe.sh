VERSION=2.0
GIT=https://github.com/xTibor/VVVVVV
BRANCH=redox
GIT_UPSTREAM=https://github.com/TerryCavanagh/VVVVVV
BUILD_DEPENDS=(sdl2_image sdl2_mixer sdl2 liborbital mesa mesa_glu zlib llvm libogg libvorbis)

function recipe_version {
    printf "1.0.0"
    skip=1
}

function recipe_build {
    sysroot="$(realpath ../sysroot)"
    cd desktop_version

    cmake \
        -DCMAKE_INSTALL_PREFIX:PATH=/ \
        -DBUILD_STATIC=ON \
        -DBUILD_SHARED=OFF \
        -DSDL2_INCLUDE_DIRS="$sysroot/include/SDL2" \
        -DSDL2_LIBRARIES="-static -lSDL2main -lSDL2_mixer -lSDL2 $("${PKG_CONFIG}" --libs glu) -lglapi -lorbital -lz -lvorbisfile -lvorbis -logg" \
        .

    make -j"$(nproc)"
    skip=1
}

function recipe_clean {
    echo "skipping clean"
    skip=1
}

function recipe_stage {
    dest="$(realpath $1)"
    mkdir -pv "$1/games/vvvvvv"
    cp ./desktop_version/VVVVVV "$1/games/vvvvvv"
    skip=1
}
