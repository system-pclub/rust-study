#!/usr/bin/env bash
set -e

source config.sh

if [ $# = 0 ]
then
    recipes="$(ls -1 recipes)"
else
    recipes="$@"
fi

for recipe in $recipes
do
    if [ -d "recipes/$recipe/source" ]
    then
        ./cook.sh "$recipe" update
    fi
done
