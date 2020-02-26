#![crate_name = "foo"]

pub trait MyTrait { fn dummy(&self) { } }

// @has foo/struct.Alpha.html '//pre' "pub struct Alpha<A>(_) where A: MyTrait"
pub struct Alpha<A>(A) where A: MyTrait;
// @has foo/trait.Bravo.html '//pre' "pub trait Bravo<B> where B: MyTrait"
pub trait Bravo<B> where B: MyTrait { fn get(&self, B: B); }
// @has foo/fn.charlie.html '//pre' "pub fn charlie<C>() where C: MyTrait"
pub fn charlie<C>() where C: MyTrait {}

pub struct Delta<D>(D);

// @has foo/struct.Delta.html '//*[@class="impl"]//code' \
//          "impl<D> Delta<D> where D: MyTrait"
impl<D> Delta<D> where D: MyTrait {
    pub fn delta() {}
}

pub struct Echo<E>(E);

// @has foo/struct.Echo.html '//*[@class="impl"]//code' \
//          "impl<E> MyTrait for Echo<E> where E: MyTrait"
// @has foo/trait.MyTrait.html '//*[@id="implementors-list"]//code' \
//          "impl<E> MyTrait for Echo<E> where E: MyTrait"
impl<E> MyTrait for Echo<E> where E: MyTrait {}

pub enum Foxtrot<F> { Foxtrot1(F) }

// @has foo/enum.Foxtrot.html '//*[@class="impl"]//code' \
//          "impl<F> MyTrait for Foxtrot<F> where F: MyTrait"
// @has foo/trait.MyTrait.html '//*[@id="implementors-list"]//code' \
//          "impl<F> MyTrait for Foxtrot<F> where F: MyTrait"
impl<F> MyTrait for Foxtrot<F> where F: MyTrait {}

// @has foo/type.Golf.html '//pre[@class="rust typedef"]' \
//          "type Golf<T> where T: Clone, = (T, T)"
pub type Golf<T> where T: Clone = (T, T);
