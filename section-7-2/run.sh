#!/usr/bin/env bash

BC_DIR="$1"
DOUBLE_LOCK_DETECTOR_LIB=DoubleLockDetector/build/lib/RustDoubleLockDetector/libRustDoubleLockDetector.so
LOG_FILE=./double_lock.log

for bc in `ls -1v ${BC_DIR}/*.m2r.bc`
do
    opt -load ${DOUBLE_LOCK_DETECTOR_LIB} -detect $bc
done 1>/dev/null 2>${LOG_FILE}
