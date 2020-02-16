#!/bin/bash
# Recommended command-line:
#
# commit-db.rb list-valid nightly|GIT_DIR=/your/rust/dir/.git ./build-src.sh

prompt_changes() {
	local MAIN_GIT_DIR="$GIT_DIR"
	local GIT_DIR=./.git CORE_IO_COMMIT=$IO_COMMIT
	git init > /dev/null
	git add .
	git commit -m "rust src import" > /dev/null
	export CORE_IO_COMMIT
	
	bold_arrow; echo 'No patch found for' $IO_COMMIT
	bold_arrow; echo 'Nearby commit(s) with patches:'
	echo
	GIT_DIR="$MAIN_GIT_DIR" git_commits_ordered '%H %cd' $(get_patch_commits) $IO_COMMIT | \
	grep --color=always -1 $IO_COMMIT | sed /$IO_COMMIT/'s/$/ <=== your commit/'
	echo
	bold_arrow; echo -e "Try applying one of those using: \e[1;36mtry_patch COMMIT\e[0m"
	bold_arrow; echo -e "Remember to test your changes with: \e[1;36mcargo build\e[0m"
	bold_arrow; echo -e "Make your changes now (\e[1;36mctrl-D\e[0m when finished)"
	bash_diff_loop "No changes were made"
	bold_arrow; echo "Saving changes as $IO_COMMIT.patch"
	git clean -f -x
	git diff > ../../patches/$IO_COMMIT.patch
	rm -rf .git
}

if [ ! -t 1 ] || [ ! -t 2 ]; then
	echo "==> /dev/stdout or /dev/stderr is not attached to a terminal!"
	echo "==> This script must be run interactively."
	exit 1
fi

cd "$(dirname "$0")"

. ./functions.sh

PATCH_DIR="$PWD/patches"
COMPILER_COMMITS=$(cat)
IO_COMMITS=$(get_io_commits|sort -u)
PATCH_COMMITS=$(get_patch_commits|sort -u)
NEW_COMMITS=$(comm -2 -3 <(echo_lines $IO_COMMITS) <(echo_lines $PATCH_COMMITS))
OLD_COMMITS=$(comm -1 -2 <(echo_lines $IO_COMMITS) <(echo_lines $PATCH_COMMITS))

set -e
set -o pipefail

find src -mindepth 1 -type d -prune -exec rm -rf {} \;

for IO_COMMIT in $OLD_COMMITS $(git_commits_ordered %H $NEW_COMMITS|tac); do
	if ! [ -d src/$IO_COMMIT ]; then
		prepare_version
		
		if [ -f patches/$IO_COMMIT.patch ]; then
			bold_arrow; echo "Patching $IO_COMMIT"
			patch -s -p1 -d src/$IO_COMMIT < patches/$IO_COMMIT.patch
		else
			cd src/$IO_COMMIT
			prompt_changes
			cd ../..
		fi
	fi
done

OLD_GIT_PERM=$(stat --printf=%a .git)
trap "chmod $OLD_GIT_PERM .git; exit 1" SIGINT
chmod 000 .git
cargo ${1:-package}
chmod $OLD_GIT_PERM .git
