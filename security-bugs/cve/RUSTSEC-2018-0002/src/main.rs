extern crate tar;

use std::fs::File;
use std::io::Write;
use std::path::Path;
use tar::{Builder, Header};
use tar::Archive;

/*
 * Reproduce:
 *      - Get tar-rs by git clone https://github.com/alexcrichton/tar-rs
 *      - git checkout c7f3b8d
 *      - Set path of tar in Cargo.toml
 *      - The assert will fail
 */
fn test() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Link);
    header.set_path("foo");
    header.set_link_name("../test");
    header.set_cksum();
    ar.append(&header, &[][..]);


    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Path::new("./testdir");
    let test = td.join("test");
    println!("test: {:?}", test);

    let mut f = match File::create(&test) {
        Ok(file) => file,
        Err(_) => File::open(&test).unwrap(),
    };
    f.write(b"sssss");

    let dir = td.join("dir");
    assert!(ar.unpack(&dir).is_err());
}

fn main() {
    test();
}
