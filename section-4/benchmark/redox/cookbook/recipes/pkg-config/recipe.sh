VERSION="0.29.2"
TAR="https://pkg-config.freedesktop.org/releases/pkg-config-${VERSION}.tar.gz"
BUILD_DEPENDS=(gettext glib libiconv pcre)

function recipe_version {
	echo "$VERSION"
	skip=1
}

function recipe_update {
	echo "skipping update"
	skip=1
}

function recipe_build {
	wget -O config.sub http://git.savannah.gnu.org/cgit/config.git/plain/config.sub
	export LDFLAGS="--static"
	./configure \
	    --build="${BUILD}" \
	    --host="${HOST}" \
	    --prefix="" \
	    --disable-shared \
	    --enable-static
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
	export DESTDIR="$(realpath $1)"
	make install
	rm -f "${DESTDIR}/lib/"*.la
	skip=1
}
