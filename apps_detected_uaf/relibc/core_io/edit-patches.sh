#!/bin/bash
# Recommended command-line:
#
# GIT_DIR=/your/rust/dir/.git ./edit-patches.sh

prompt_changes() {
	bold_arrow; echo "Editing $IO_COMMIT"
	bold_arrow; echo -e "Remember to test your changes with: \e[1;36mcargo build\e[0m"

	local MAIN_GIT_DIR="$GIT_DIR"
	local GIT_DIR=./.git CORE_IO_COMMIT=$IO_COMMIT
	export CORE_IO_COMMIT

	git init > /dev/null
	git add .
	git commit -m "rust src import" > /dev/null
	IMPORT_COMMIT=$(git log -n1 --pretty=format:%H)
	patch -s -p1 < $PATCH_DIR/$IO_COMMIT.patch
	git commit -a -m "existing patch for $IO_COMMIT" > /dev/null

	bold_arrow; echo -e "Applying patch from \e[1;36m$TMP_PATCH\e[0m"
	patch -p1 < $TMP_PATCH || true
	bold_arrow; echo -e "Make your changes now (\e[1;36mctrl-D\e[0m when finished)"
	bash_diff_loop "No changes were made"
	bold_arrow; echo "Replacing $IO_COMMIT.patch with updated version"
	git diff > $TMP_PATCH
	git clean -f -x
	git diff > $PATCH_DIR/$IO_COMMIT.patch
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
PATCH_COMMITS=$(get_patch_commits|sort -u)

TMP_PATCH=$(mktemp)

set -e
set -o pipefail

find src -mindepth 1 -type d -prune -exec rm -rf {} \;

for IO_COMMIT in $(git_commits_ordered %H $PATCH_COMMITS|tac); do
	prepare_version
	cd src/$IO_COMMIT
	prompt_changes
	cd ../..
done

rm -rf $TMP_PATCH

bold_arrow; echo "Done"
