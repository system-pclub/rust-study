GIT=https://gitlab.redox-os.org/redox-os/liborbital.git

function recipe_stage {
    dest="$(realpath $1)"
    make HOST="$HOST" DESTDIR="$dest" install
    skip=1
}
