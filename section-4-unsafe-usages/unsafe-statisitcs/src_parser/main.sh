#!/usr/bin/env bash

function count_one_file() {
    comment_remover="comment_remover/comment_remover"    
    unsafe_block_extractor="unsafe_block_extractor/unsafe_block_extractor.py"
    unsafe_fn_extractor="unsafe_fn_extractor/unsafe_fn_extractor.py"
    
    $comment_remover $1 > tmp
    $unsafe_block_extractor tmp >> unsafe_block.info
    $unsafe_fn_extractor tmp >> unsafe_fn.info
    grep 'unsafe trait .*{' tmp >> unsafe_trait.info
}

function main() {
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
    echo "# and LOC of unsafe block:"
    ./sum.py unsafe_block.info
    echo "# and LOC of unsafe fn:"
    ./sum.py unsafe_fn.info
    echo "# of unsafe trait:"
    wc -l unsafe_trait.info | cut -d$' ' -f1
}

main "$1"
