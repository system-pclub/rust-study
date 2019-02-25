pub fn starts_with_ignore_ascii_case_panic(string: &str, prefix: &str) -> bool {
    println!("string len: {}, prefix len: {}", string.len(), prefix.len());
    string.len() > prefix.len() &&
        string[0..prefix.len()].eq_ignore_ascii_case(prefix)
}

pub fn starts_with_ignore_ascii_case_patch(string: &str, prefix: &str) -> bool {
    println!("string len: {}, prefix len: {}", string.len(), prefix.len());
    string.len() > prefix.len() &&
        string.as_bytes()[0..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn main() {
    /*
     * Note that you probably see the "aaaaa" here, actually there are some no ASCII case
     * at the end will not be displayed
     */
    starts_with_ignore_ascii_case_patch("aaaaa💩", "-webkit-");
    starts_with_ignore_ascii_case_panic("aaaaa💩", "-webkit-");
}
