use std::env;

fn main() {
    println!("cargo:rustc-env=PKG_DEFAULT_TARGET={}", env::var("TARGET").unwrap());
}
