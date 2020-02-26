#![feature(no_core, lang_items)]
#![crate_type="rlib"]
#![no_core]

pub static STATIC_BOOL: bool = true;

pub static mut STATIC_MUT_BOOL: bool = true;

const CONST_BOOL: bool = true;
pub static CONST_BOOL_REF: &'static bool = &CONST_BOOL;


#[lang = "sized"]
trait Sized {}

#[lang = "copy"]
trait Copy {}

#[lang = "freeze"]
trait Freeze {}

#[lang = "sync"]
trait Sync {}
impl Sync for bool {}
impl Sync for &'static bool {}

#[lang="drop_in_place"]
pub unsafe fn drop_in_place<T: ?Sized>(_: *mut T) { }
