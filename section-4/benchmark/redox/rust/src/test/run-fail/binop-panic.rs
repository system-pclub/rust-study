// error-pattern:quux
fn my_err(s: String) -> ! {
    println!("{}", s);
    panic!("quux");
}
fn main() {
    3_usize == my_err("bye".to_string());
}
