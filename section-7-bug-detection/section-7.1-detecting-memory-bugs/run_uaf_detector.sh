#!/bin/sh
cd ../applications/relibc
cargo clean && cargo rustc -- -Zdump-mir="PreCodegen"
cd ../../section-7.1-detecting-memory-bugs/use-after-free-detector
python3 main.py ../../applications/relibc/mir_dump
