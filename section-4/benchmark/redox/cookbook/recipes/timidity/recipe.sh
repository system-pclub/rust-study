VERSION=2.15.0
GIT=https://gitlab.redox-os.org/redox-os/timidity.git
BRANCH=redox
DEPENDS="generaluser-gs"

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
    autoreconf -f -i
    wget -O autoconf/config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub
    ./configure \
        --build=${BUILD} \
        --host=${HOST} \
        --prefix='' \
        --enable-vt100
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

    mkdir -pv "$1/share/timidity"
    echo "soundfont /share/generaluser-gs/generaluser-gs.sf2" >> "$1/share/timidity/timidity.cfg"

    skip=1
}
