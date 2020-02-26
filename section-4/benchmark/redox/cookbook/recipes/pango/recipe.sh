VERSION="1.42.4"
TAR="ftp.gnome.org/pub/GNOME/sources/pango/${VERSION%.*}/pango-${VERSION}.tar.xz"
BUILD_DEPENDS=(cairo expat fontconfig freetype fribidi gettext glib harfbuzz libffi libiconv libpng pcre pixman zlib)

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
	sysroot="$(realpath ../sysroot)"
	export CFLAGS="-I$sysroot/include"
	export LDFLAGS="-L$sysroot/lib --static"
	export GLIB_MKENUMS="$sysroot/bin/glib-mkenums"
	./configure \
	    --build=${BUILD} \
	    --host=${HOST} \
	    --prefix=/ \
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
	dest="$(realpath $1)"
	make DESTDIR="$dest" install
	rm -f "$dest/lib/"*.la
	skip=1
}
