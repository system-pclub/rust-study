extern crate rustc_version;

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::ops::{Neg,Sub};

/*
 * Let me explain this hack. For the sync shell script it's easiest if every 
 * line in mapping.rs looks exactly the same. This means that specifying an 
 * array literal is not possible. include!() can only expand to expressions, so 
 * just specifying the contents of an array is also not possible.
 *
 * This leaves us with trying to find an expression in which every line looks 
 * the same. This can be done using the `-` operator. This can be a unary 
 * operator (first thing on the first line), or a binary operator (later 
 * lines). That is exactly what's going on here, and Neg and Sub simply build a 
 * vector of the operangs.
 */
struct Mapping(&'static str,&'static str);

impl Neg for Mapping {
	type Output = Vec<Mapping>;
    fn neg(self) -> Vec<Mapping> {
		vec![self.into()]
	}
}

impl Sub<Mapping> for Vec<Mapping> {
    type Output=Vec<Mapping>;
    fn sub(mut self, rhs: Mapping) -> Vec<Mapping> {
		self.push(rhs.into());
		self
	}
}

fn main() {
	let ver=rustc_version::version_meta();

	let io_commit="b9adc3327ec7d2820ab2db8bb3cc2a0196a8375d";
	/*
	let io_commit=match env::var("CORE_IO_COMMIT") {
		Ok(c) => c,
		Err(env::VarError::NotUnicode(_)) => panic!("Invalid commit specified in CORE_IO_COMMIT"),
		Err(env::VarError::NotPresent) => {
			let mappings=include!("mapping.rs");
			
			let compiler=ver.commit_hash.expect("Couldn't determine compiler version");
			mappings.iter().find(|&&Mapping(elem,_)|elem==compiler).expect("Unknown compiler version, upgrade core_io?").1.to_owned()
		}
	};
	*/

	if ver.commit_date.as_ref().map_or(true,|d| &**d>="2018-01-01") {
		println!("cargo:rustc-cfg=core_memchr");
	}

	if ver.commit_date.as_ref().map_or(true,|d| &**d>="2017-06-15") {
		println!("cargo:rustc-cfg=no_collections");
	}

	if ver.commit_date.as_ref().map_or(false,|d| &**d<"2016-12-15") {
		println!("cargo:rustc-cfg=rustc_unicode");
	} else if ver.commit_date.as_ref().map_or(false,|d| &**d<"2017-03-03") {
		println!("cargo:rustc-cfg=std_unicode");
	}

	let mut dest_path=PathBuf::from(env::var_os("OUT_DIR").unwrap());
	dest_path.push("io.rs");
	let mut f=File::create(&dest_path).unwrap();
	
	let mut target_path=PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
	target_path.push("src");
	target_path.push(io_commit);
	target_path.push("mod.rs");

	f.write_all(br#"#[path=""#).unwrap();
	f.write_all(target_path.into_os_string().into_string().unwrap().replace("\\", "\\\\").as_bytes()).unwrap();
	f.write_all(br#""] mod io;"#).unwrap();
}
