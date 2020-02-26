VERSION=3.6
TAR=http://ftp.gnu.org/gnu/diffutils/diffutils-$VERSION.tar.xz

function recipe_version {
    echo "$VERSION"
    skip=1
}

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_build {
    export LDFLAGS="-static"
    autoreconf
    ./configure \
        --build=${BUILD} \
        --host=${HOST} \
        --prefix=/ \
        gt_cv_locale_fr=false \
        gt_cv_locale_fr_utf8=false \
        gt_cv_locale_ja=false \
        gt_cv_locale_tr_utf8=false \
        gt_cv_locale_zh_CN=false
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
    ${HOST}-strip "$dest"/bin/*
    rm -rf "$dest"/{lib,share}
    skip=1
}
