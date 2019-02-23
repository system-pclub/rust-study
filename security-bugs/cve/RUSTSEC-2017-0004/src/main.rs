/**
 *  This bug need to be reproduced in release build.
 */

fn mock_encode_size_buggy(bytes_len: usize) -> usize {
    let rem = bytes_len % 3;

    let complete_input_chunks = bytes_len / 3;
    let complete_output_chars = complete_input_chunks * 4;
    let printing_output_chars = if rem == 0 {
        complete_output_chars
    } else {
        complete_output_chars + 4
    };
    let line_ending_output_chars = printing_output_chars * 2;

    return printing_output_chars + line_ending_output_chars;
}

fn mock_encoded_size_patch(bytes_len: usize) -> Option<usize> {
    let printing_output_chars = bytes_len
        .checked_add(2)
        .map(|x| x / 3)
        .and_then(|x| x.checked_mul(4));

    let line_ending_output_chars = printing_output_chars.and_then(|y| y.checked_mul(2));

    printing_output_chars.and_then(|x|
        line_ending_output_chars.and_then(|y| x.checked_add(y)))
}


fn main() {
    let bytes_len = 1 << 63;
    let mut ret = mock_encode_size_buggy(bytes_len);
    println!("buggy ret: {}", ret);
    let resv_size = match mock_encoded_size_patch(bytes_len) {
        Some(ret) => {
            println!("patch ret: {}", ret);
        },
        None => panic!("integer overflow when calculating buffer size"),
    };

    // If you use the ret as a hint to allocate memory, it can lead to memory corruption
}
