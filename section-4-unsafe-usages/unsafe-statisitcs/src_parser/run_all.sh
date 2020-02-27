#!/usr/bin/env bash

count_dir=/home/rust/Projects/count

g_unsafe_region_num=0
g_unsafe_region_LOC=0
g_unsafe_fn_num=0
g_unsafe_fn_LOC=0
g_unsafe_total_LOC=0
g_unsafe_trait_num=0
g_total_LOC=0

unsafe_region_num=0
unsafe_region_LOC=0
unsafe_fn_num=0
unsafe_fn_LOC=0
unsafe_total_LOC=0
unsafe_trait_num=0
total_LOC=0

function g_reset() {
    g_unsafe_region_num=0
    g_unsafe_region_LOC=0
    g_unsafe_fn_num=0
    g_unsafe_fn_LOC=0
    g_unsafe_total_LOC=0
    g_unsafe_trait_num=0
    g_total_LOC=0
}

function reset() {
    let g_unsafe_region_num+=unsafe_region_num
    let g_unsafe_region_LOC+=unsafe_region_LOC
    let g_unsafe_fn_num+=unsafe_fn_num
    let g_unsafe_fn_LOC+=unsafe_fn_LOC
    let g_unsafe_total_LOC+=unsafe_total_LOC
    let g_unsafe_trait_num+=unsafe_trait_num
    let g_total_LOC+=total_LOC
    unsafe_region_num=0
    unsafe_region_LOC=0
    unsafe_fn_num=0
    unsafe_fn_LOC=0
    unsafe_total_LOC=0
    unsafe_trait_num=0
    total_LOC=0
}

function count_one_file() {
    comment_remover="comment_remover/comment_remover"
    unsafe_block_extractor="unsafe_block_extractor/unsafe_block_extractor.py"
    unsafe_fn_extractor="unsafe_fn_extractor/unsafe_fn_extractor.py"

    $comment_remover $1 > tmp
    $unsafe_block_extractor tmp >> unsafe_block.info
    $unsafe_fn_extractor tmp >> unsafe_fn_LOC.info
    egrep 'unsafe (\w )*fn .+\(' tmp | grep -v ';$' >> unsafe_fn.info
    grep 'unsafe trait .*{' tmp >> unsafe_trait.info
    let total_LOC+=$(cat tmp | grep -v '^$' | wc -l | cut -d' ' -f1)
}

function count_one_file_fast() {
    comment_remover="comment_remover/comment_remover"
    unsafe_block_extractor="unsafe_block_extractor/unsafe_block_extractor.py"
    unsafe_fn_extractor="unsafe_fn_extractor/unsafe_fn_extractor.py"

    $comment_remover $1 > tmp
    $unsafe_block_extractor tmp >> unsafe_block.info
    egrep 'unsafe (\w )*fn .+\(' tmp | grep -v ';$' >> unsafe_fn.info
    grep 'unsafe trait .*{' tmp >> unsafe_trait.info
}

function count_dir() {
    rm tmp unsafe_block.info unsafe_fn.info unsafe_fn_LOC.info unsafe_trait.info
    input_dir="$1"
    array=()
    while IFS=  read -r -d $'\0'; do
        array+=("$REPLY")
    done < <(find "${input_dir}" -type f -name "*.rs" -print0)

    for i in "${array[@]}"; do
        #echo $i
        count_one_file "$i"
    done
    #echo "# and LOC of unsafe block:"
    unsafe_regions=$(./sum.py unsafe_block.info)
    unsafe_region_num=$(echo $unsafe_regions | cut -d' ' -f1)
    unsafe_region_LOC=$(echo $unsafe_regions | cut -d' ' -f2)
    #echo "# and LOC of unsafe fn:"
    unsafe_fn_num=$(wc -l unsafe_fn.info | cut -d' ' -f1)
    unsafe_fn_LOC=$(./sum.py unsafe_fn_LOC.info | cut -d' ' -f2)
    let unsafe_fn_LOC+=unsafe_fn_num
    let unsafe_total_LOC=unsafe_region_LOC+unsafe_fn_LOC
    #echo "# of unsafe trait:"
    unsafe_trait_num=$(wc -l unsafe_trait.info | cut -d' ' -f1)
}

function count_dir_fast() {
    rm tmp unsafe_block.info unsafe_fn.info unsafe_trait.info
    input_dir="$1"
    array=()
    while IFS=  read -r -d $'\0'; do
        array+=("$REPLY")
    done < <(find "${input_dir}" -type f -name "*.rs" -print0)

    for i in "${array[@]}"; do
        #echo $i
        count_one_file "$i"
    done
    #echo "# and LOC of unsafe block:"
    unsafe_regions=$(./sum.py unsafe_block.info)
    unsafe_region_num=$(echo $unsafe_regions | cut -d' ' -f1)
    #echo "# and LOC of unsafe fn:"
    unsafe_fn_num=$(wc -l unsafe_fn.info | cut -d' ' -f1)
    #echo "# of unsafe trait:"
    unsafe_trait_num=$(wc -l unsafe_trait.info | cut -d' ' -f1)
}

function parse_libstd() {
    count_dir_fast ${count_dir}/rust/src/libstd/
    unsafe_fn_tp=15
    unsafe_region_fp=4
    let unsafe_fn_num+=unsafe_fn_tp
    let unsafe_region_num-=unsafe_region_fp
    #echo "URN,UFN,UTN,URLOC,UFLOC,TLOC:"
    #echo "URN,UFN,UTN:"
    #echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_region_LOC} ${unsafe_fn_LOC} ${total_LOC}
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_libcore() {
    count_dir_fast ${count_dir}/rust/src/libcore/
    unsafe_fn_fp=3
    let unsafe_fn_num-=unsafe_fn_fp
    #echo "URN,UFN,UTN:"
    #echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_region_LOC} ${unsafe_fn_LOC} ${total_LOC}
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_liballoc() {
    count_dir_fast ${count_dir}/rust/src/liballoc/
    let unsafe_fn_num-=unsafe_fn_fp
    #echo "URN,UFN,UTN:"
    #echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_region_LOC} ${unsafe_fn_LOC} ${total_LOC}
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function count_std() {
   g_reset
   echo "unsafe region num, unsafe fn num, unsafe trait num:"
   reset
   echo "libstd"
   parse_libstd
   reset
   echo "libcore"
   parse_libcore
   reset
   echo "liballoc"
   parse_liballoc
   echo "total std"
   echo ${g_unsafe_fn_num} ${g_unsafe_region_num} ${g_unsafe_trait_num}
}

function parse_rand() {
    count_dir_fast ${count_dir}/rand
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_crossbeam() {
    count_dir_fast ${count_dir}/crossbeam
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_threadpool() {
    count_dir_fast ${count_dir}/rust-threadpool
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_rayon() {
    count_dir_fast ${count_dir}/rayon
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_lazy_static() {
    count_dir_fast ${count_dir}/lazy-static.rs
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_servo() {
    count_dir_fast ${count_dir}/servo
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_tikv() {
    count_dir_fast ${count_dir}/tikv
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_ethereum() {
    count_dir_fast ${count_dir}/parity-ethereum
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_redox_rust() {
   # count_dir_fast ${count_dir}/redox_rust
   let unsafe_fn_num+=$(cat ${count_dir}/redox_rust/.info | cut -d' ' -f1)
   let unsafe_region_num+=$(cat ${count_dir}/redox_rust/.info | cut -d' ' -f2)
   let unsafe_trait_num+=$(cat ${count_dir}/redox_rust/.info | cut -d' ' -f3)
}

function parse_redox() {
    count_dir_fast ${count_dir}/redox
    # precompute redox_rust
    parse_redox_rust
    let unsafe_fn_num+=redox_rust_fn
    let unsafe_region_num+=redox_rust_region
    let unsafe_trait_num+=redox_rust_trait
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function parse_tock() {
    count_dir_fast ${count_dir}/tock
    echo ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num}
}

function count_apps() { 
   g_reset
   echo "unsafe region num, unsafe fn num, unsafe trait num:"
   reset
   echo "rand"
   parse_rand
   reset
   echo "crossbeam"
   parse_crossbeam
   reset
   echo "threadpool"
   parse_threadpool
   reset
   echo "rayon"
   parse_rayon
   reset
   echo "lazy-static.rs"
   parse_lazy_static
   echo "libs:"
   echo ${g_unsafe_fn_num} ${g_unsafe_region_num} ${g_unsafe_trait_num}
   reset
   echo "servo:"
   parse_servo
   reset
   echo "tikv:"
   parse_tikv
   reset
   echo "ethereum:"
   parse_ethereum
   reset
   echo "redox:"
   parse_redox
   reset
   echo "tock:"
   parse_tock
   echo "All:"
   echo ${g_unsafe_fn_num} ${g_unsafe_region_num} ${g_unsafe_trait_num}
}


function count_std_pub_mod() {
    g_reset
    echo "unsafe region num, unsafe fn num, unsafe trait num, unsafe_total_LOC, total_LOC"
    reset
    count_dir ${count_dir}/rust/src/libstd/collections/
    echo "collections" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/io/
    echo "io" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/net/
    echo "net" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/os/
    echo "os" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/sync/
    echo "sync" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/sys/
    echo "sys" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
    reset
    count_dir ${count_dir}/rust/src/libstd/thread/
    echo "thread" ${unsafe_fn_num} ${unsafe_region_num} ${unsafe_trait_num} ${unsafe_total_LOC} ${total_LOC}
}

count_std
count_apps
count_std_pub_mod
