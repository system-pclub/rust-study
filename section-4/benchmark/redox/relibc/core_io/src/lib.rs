//! <p id="core_io-show-docblock"></p>
//! This is just a listing of the functionality available in this crate. See
//! the [std documentation](https://doc.rust-lang.org/nightly/std/io/index.html)
//! for a full description of the functionality.
#![allow(stable_features,unused_features)]
#![feature(question_mark,const_fn,copy_from_slice,
	try_from,str_internals,align_offset,doc_spotlight,slice_internals)]
#![cfg_attr(feature="alloc",feature(alloc))]
#![no_std]

#[cfg_attr(feature="collections",macro_use)]
#[cfg(all(feature="collections",not(no_collections)))] extern crate collections;
#[cfg_attr(feature="collections",allow(unused_imports))]
#[cfg_attr(feature="collections",macro_use)]
#[cfg(all(feature="collections",no_collections))] extern crate alloc as collections;
#[cfg(feature="alloc")] extern crate alloc;
#[cfg(rustc_unicode)]
extern crate rustc_unicode;
#[cfg(std_unicode)]
extern crate std_unicode;

#[cfg(not(feature="collections"))]
pub type ErrorString = &'static str;

// Provide Box::new wrapper
#[cfg(not(feature="alloc"))]
struct FakeBox<T>(core::marker::PhantomData<T>);
#[cfg(not(feature="alloc"))]
impl<T> FakeBox<T> {
	fn new(val: T) -> T {
		val
	}
}

// Needed for older compilers, to ignore vec!/format! macros in tests
#[cfg(not(feature="collections"))]
#[allow(unused)]
macro_rules! vec (
	( $ elem : expr ; $ n : expr ) => { () };
	( $ ( $ x : expr ) , * ) => { () };
	( $ ( $ x : expr , ) * ) => { () };
);
#[cfg(not(feature="collections"))]
#[allow(unused)]
macro_rules! format {
	( $ ( $ arg : tt ) * ) => { () };
}

include!(concat!(env!("OUT_DIR"), "/io.rs"));
pub use io::*;
