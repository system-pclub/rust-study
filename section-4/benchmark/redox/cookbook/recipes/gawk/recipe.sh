GIT=https://gitlab.redox-os.org/redox-os/gawk.git
GIT_UPSTREAM=https://git.savannah.gnu.org/git/gawk.git
BRANCH=redox

function recipe_update {
    echo "skipping update"
    skip=1
}

function recipe_build {
    ./configure --build=${BUILD} --host=${HOST} --prefix=/ ac_cv_func_gethostbyname=no ac_cv_func_connect=no
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
    skip=1
}
