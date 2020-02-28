#!/usr/bin/env bash

BC_DIR="$1"

# build
cd DoubleLockDetector
mkdir -p build
cd build
cmake ..
make
cd ../..

# run
for APP in ${BC_DIR}/*
do
        if [ -d ${APP} ]
        then
                echo ${APP}
                ./run.sh ${APP}/m2r
        fi
done
