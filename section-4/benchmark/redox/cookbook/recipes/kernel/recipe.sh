GIT=https://gitlab.redox-os.org/redox-os/kernel.git
BUILD_DEPENDS=(drivers init nulld randd redoxfs zerod)

function recipe_build {
    export INITFS_FOLDER="$(realpath ../sysroot)"
    mkdir -pv "$INITFS_FOLDER/etc"
    cp -v "$(realpath ../init.rc)" "$INITFS_FOLDER/etc/init.rc"
    xargo rustc \
        --lib \
        --target "${ARCH}-unknown-none" \
        --release \
        -- \
        -C soft-float \
        -C debuginfo=2 \
        --emit link=libkernel.a
    "${LD}" \
        --gc-sections \
        -z max-page-size=0x1000 \
        -T "linkers/${ARCH}.ld" \
        -o kernel \
        libkernel.a
    "${OBJCOPY}" \
        --only-keep-debug \
        kernel \
        kernel.sym
    "${OBJCOPY}" \
        --strip-debug \
        kernel
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
    cp -v kernel "$dest"
    skip=1
}
