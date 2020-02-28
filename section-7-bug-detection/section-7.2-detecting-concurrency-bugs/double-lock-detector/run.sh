#!/usr/bin/env bash

BC_DIR="$1"
DOUBLE_LOCK_DETECTOR_LIB=DoubleLockDetector/build/lib/RustDoubleLockDetector/libRustDoubleLockDetector.so
LOG_FILE=./double_lock.log

for bc in `ls -1v ${BC_DIR}/*.m2r.bc`
do
    #echo $bc
    opt -load ${DOUBLE_LOCK_DETECTOR_LIB} -detect $bc 1>/dev/null
done 2>>${LOG_FILE}
