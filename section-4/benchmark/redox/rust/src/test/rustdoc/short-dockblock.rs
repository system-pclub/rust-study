#![crate_name = "foo"]

// @has foo/index.html '//*[@class="docblock-short"]/p' 'fooo'
// @!has foo/index.html '//*[@class="docblock-short"]/p/h1' 'fooo'
// @has foo/fn.foo.html '//h1[@id="fooo"]/a[@href="#fooo"]' 'fooo'

/// # fooo
///
/// foo
pub fn foo() {}

// @has foo/index.html '//*[@class="docblock-short"]/p' 'mooood'
// @!has foo/index.html '//*[@class="docblock-short"]/p/h2' 'mooood'
// @has foo/foo/index.html '//h2[@id="mooood"]/a[@href="#mooood"]' 'mooood'

/// ## mooood
///
/// foo mod
pub mod foo {}

// @has foo/index.html '//*[@class="docblock-short"]/p/a[@href=\
//                      "https://nougat.world"]/code' 'nougat'

/// [`nougat`](https://nougat.world)
pub struct Bar;
