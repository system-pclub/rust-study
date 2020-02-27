#!/usr/bin/env bash

BC_DIR="$1"
MANUAL_DROP_LIB=ManualDropPrinter/build/lib/PrintManualDrop/libPrintManualDrop.so
LOG_FILE=./manual_drop.log

for bc in `ls -1v ${BC_DIR}/*.m2r.bc`
do
    opt -load ${MANUAL_DROP_LIB} -print $bc
done 1>/dev/null 2>${LOG_FILE}

./parse_manual_drop_log.py ${LOG_FILE}
