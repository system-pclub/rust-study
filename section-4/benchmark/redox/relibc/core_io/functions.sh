#!/bin/bash

git_file_exists() {
	[ "$(git ls-tree --name-only $IO_COMMIT -- $1)" = "$1" ]
}

git_extract() {
	slashes=${1//[^\/]/}
	git archive $IO_COMMIT $1|tar xf - -C src/$IO_COMMIT --strip-components=${#slashes}
}

git_commits_ordered() {
	format="$1"
	shift
	if [ $# -ge 1 ]; then
		git log --topo-order --no-walk=sorted --date=iso-local --pretty=format:"$format" "$@"
	fi
	echo
}

echo_lines() {
	for i in "$@"; do
		echo $i
	done
}

get_io_commits() {
	for COMPILER_COMMIT in $COMPILER_COMMITS; do
		IO_COMMIT=$(git log -n1 --pretty=format:%H $COMPILER_COMMIT -- src/libstd/io)
		if ! grep -q $COMPILER_COMMIT mapping.rs; then
			echo "-Mapping(\"$COMPILER_COMMIT\",\"$IO_COMMIT\")" >> mapping.rs
		fi
		echo $IO_COMMIT
	done
}

get_patch_commits() {
	find $PATCH_DIR -type f -printf %f\\n|cut -d. -f1
}

prepare_version() {
	mkdir src/$IO_COMMIT
	git_extract src/libstd/io/
	if git_file_exists src/libcore/slice/memchr.rs; then
		true
	elif git_file_exists src/libstd/sys_common/memchr.rs; then
		git_extract src/libstd/sys_common/memchr.rs
	elif git_file_exists src/libstd/sys/common/memchr.rs; then
		git_extract src/libstd/sys/common/memchr.rs
	else
		git_extract src/libstd/memchr.rs
	fi
	rm -f src/$IO_COMMIT/stdio.rs src/$IO_COMMIT/lazy.rs
}

bold_arrow() {
	echo -ne '\e[1;36m==> \e[0m'
}

custom_bashrc() {
	echo '
if [ -f ~/.bashrc ]; then . ~/.bashrc; fi

try_patch() {
	patch -p1 < ../../patches/$1.patch
}
'
}

bash_diff_loop() {
	bash --rcfile <(custom_bashrc) <> /dev/stderr
	while git diff --exit-code > /dev/null; do
		bold_arrow; echo "$1"
		while true; do
			bold_arrow; echo -n "(T)ry again or (A)bort? "
			read answer <> /dev/stderr
			case "$answer" in
				[tT])
					break
					;;
				[aA])
					bold_arrow; echo "Aborting..."
					exit 1
					;;
			esac
		done
		bash <> /dev/stderr
	done
}
