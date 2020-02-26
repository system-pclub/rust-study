// normalize-stderr-64bit "18446744073709551615" -> "SIZE"
// normalize-stderr-32bit "4294967295" -> "SIZE"

// error-pattern: is too big for the current architecture
fn main() {
    println!("Size: {}", std::mem::size_of::<[u8; std::u64::MAX as usize]>());
}
