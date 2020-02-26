# termsize

[![Build Status](https://travis-ci.org/softprops/termsize.svg)](https://travis-ci.org/softprops/termsize) [![Build status](https://ci.appveyor.com/api/projects/status/ilics7dppw0vl6gb?svg=true)](https://ci.appveyor.com/project/softprops/termsize) [![Coverage Status](https://coveralls.io/repos/softprops/termsize/badge.svg?branch=master&service=github)](https://coveralls.io/github/softprops/termsize?branch=master)

> because terminal size matters

Termsize is a rust crate providing a multi-platform interface for resolving
your terminal's current size in rows and columns. On most unix systems, this is similar invoking the [stty(1)](http://man7.org/linux/man-pages/man1/stty.1.html) program, requesting the terminal size.


## [Documentation](https://softprops.github.com/termsize)

## install

add the following to your `Cargo.toml` file

```toml
[dependencies]
termsize = "0.1"
```

## usage

Termize provides one function, `get`, which returns a `termsize::Size` struct
exposing two fields: `rows` and `cols` representing the number of rows and columns
a a terminal's stdout supports.

```rust
extern crate termsize;

pub fn main() {
  termsize::get().map(|size| {
    println!("rows {} cols {}", size.rows, size.cols)
  });
}
```

Doug Tangren (softprops) 2015-2017