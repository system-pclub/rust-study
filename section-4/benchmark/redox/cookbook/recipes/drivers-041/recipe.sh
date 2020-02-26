GIT=https://gitlab.redox-os.org/redox-os/drivers.git
BRANCH=0.4.1
CARGOFLAGS=--all

function recipe_version {
    echo "0.1.1"
    skip=1
}

function recipe_stage {
    mkdir -pv "$1/etc/pcid"
    cp -v initfs.toml "$1/etc/pcid/initfs.toml"
    cp -v filesystem.toml "$1/etc/pcid/filesystem.toml"
}
