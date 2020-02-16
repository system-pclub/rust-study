#!/bin/bash
cd "$(dirname "$0")"
HIDE_DOCBLOCKS='.docblock>*, .collapse-toggle, #toggle-all-docs { display: none; } #core_io-show-docblock+p { display: initial }'
FIX_ERRORSTRING='.method a.type[title="core_io::ErrorString"]:before { content: "Error"; }'
rm -rf target/doc
cargo rustdoc --features collections -- --html-in-header <(echo '<style type="text/css">'"$HIDE_DOCBLOCKS"'</style>')
mv target/doc target/doc_collections
cargo rustdoc --features alloc -- --html-in-header <(echo '<style type="text/css">'"$HIDE_DOCBLOCKS $FIX_ERROR_STRING"'</style>')
mv target/doc target/doc_alloc
cargo rustdoc -- --html-in-header <(echo '<style type="text/css">'"$HIDE_DOCBLOCKS $FIX_ERROR_STRING"'</style>')
