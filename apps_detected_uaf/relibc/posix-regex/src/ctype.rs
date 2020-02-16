pub fn is_alnum(c: u8) -> bool {
    is_alpha(c) || is_digit(c)
}
pub fn is_alpha(c: u8) -> bool {
    is_lower(c) || is_upper(c)
}
pub fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}
pub fn is_cntrl(c: u8) -> bool {
    c <= 0x1f || c == 0x7f
}
pub fn is_digit(c: u8) -> bool {
    c >= b'0' && c <= b'9'
}
pub fn is_graph(c: u8) -> bool {
    c >= 0x21 && c <= 0x7e
}
pub fn is_lower(c: u8) -> bool {
    c >= b'a' && c <= b'z'
}
pub fn is_print(c: u8) -> bool {
    c >= 0x20 && c <= 0x7e
}
pub fn is_punct(c: u8) -> bool {
    is_graph(c) && !is_alnum(c)
}
pub fn is_space(c: u8) -> bool {
    c == b' ' || (c >= 0x9 && c <= 0xD)
}
pub fn is_upper(c: u8) -> bool {
    c >= b'A' && c <= b'Z'
}
pub fn is_xdigit(c: u8) -> bool {
    is_digit(c) || (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F')
}

pub fn is_word_boundary(c: u8) -> bool {
    !is_alnum(c) && c != b'_'
}
