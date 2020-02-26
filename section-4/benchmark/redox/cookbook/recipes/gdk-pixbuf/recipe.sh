VERSION="2.38.1"
TAR="https://ftp.gnome.org/pub/gnome/sources/gdk-pixbuf/${VERSION%.*}/gdk-pixbuf-${VERSION}.tar.xz"
BUILD_DEPENDS=(gettext glib libffi libiconv libjpeg libpng pcre shared-mime-info zlib)

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
	export LDFLAGS="-static"

	# TODO: Fix this annoying shite
	echo "[binaries]" > cross_file.txt
	echo "c = '${CC}'" >> cross_file.txt
	echo "cpp = '${CXX}'" >> cross_file.txt
	echo "ar = '${AR}'" >> cross_file.txt
	echo "strip = '${STRIP}'" >> cross_file.txt
	echo "pkgconfig = '${PKG_CONFIG}'" >> cross_file.txt

	echo "[host_machine]" >> cross_file.txt
	echo "system = 'redox'" >> cross_file.txt
	echo "cpu_family = 'x86_64'" >> cross_file.txt
	echo "cpu = 'x86_64'" >> cross_file.txt
	echo "endian = 'little'" >> cross_file.txt

	echo "[paths]" >> cross_file.txt
	echo "prefix = '${sysroot}'" >> cross_file.txt
	echo "libdir = 'lib'" >> cross_file.txt
	echo "bindir = 'bin'" >> cross_file.txt

	unset AR
	unset AS
	unset CC
	unset CXX
	unset LD
	unset NM
	unset OBJCOPY
	unset OBJDUMP
	unset PKG_CONFIG
	unset RANLIB
	unset READELF
	unset STRIP

	meson . _build \
	    --cross-file cross_file.txt \
	    --buildtype release \
	    --strip \
	    -Ddefault_library=static \
	    -Dprefix=/ \
	    -Dlibdir=lib \
	    -Dbuiltin_loaders=all \
	    -Dgir=false \
	    -Dx11=false
	ninja -C _build -v
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
	DESTDIR="$dest" ninja -C _build -v install
	rm -f "$dest/lib/"*.la
	skip=1
}
