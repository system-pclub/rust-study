fn next_power_of_two(mut v: u32) -> u32 {
    v -= 1;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v += 1;
    v
}


fn main() {
    let num : u32 = 0;
    let bug = next_power_of_two(num);
    let patch = num.next_power_of_two();
    println!("bug: {}, patch: {}", bug, patch);
}
