VERSION="1.10"
TAR="https://freedesktop.org/~hadess/shared-mime-info-${VERSION}.tar.xz"
BUILD_DEPENDS=(gettext glib libffi libiconv libxml2 pcre zlib)

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
	export CFLAGS="-I$sysroot/include"
	export LDFLAGS="-L$sysroot/lib --static"
	./configure \
	    --build=${BUILD} \
	    --host=${HOST} \
	    --prefix=/
	make
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
	rm -f "$dest/lib/"*.la
	skip=1
}
