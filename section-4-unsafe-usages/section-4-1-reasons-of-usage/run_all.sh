#!/usr/bin/env bash

cd mem-copy
cargo bench 2>/dev/null

cd ../array-access  
cargo bench 2>/dev/null

cd ../array-offset
cargo bench 2>/dev/null
