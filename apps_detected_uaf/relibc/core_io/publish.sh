#!/bin/bash
OLD_GIT_PERM=$(stat --printf=%a .git)
trap "chmod $OLD_GIT_PERM .git; exit 1" SIGINT
chmod 000 .git
cargo publish
chmod $OLD_GIT_PERM .git
