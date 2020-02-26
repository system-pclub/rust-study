# Creating a new project

When you create a new project with Cargo, it will automatically add
configuration for the latest edition:

```console
> cargo +nightly new foo
     Created binary (application) `foo` project
> cat .\foo\Cargo.toml
[package]
name = "foo"
version = "0.1.0"
authors = ["your name <you@example.com>"]
edition = "2018"

[dependencies]
```

That `edition = "2018"` setting will configure your package to use Rust 2018.
No more configuration needed!

If you'd prefer to use an older edition, you can change the value in that
key, for example:

```toml
[package]
name = "foo"
version = "0.1.0"
authors = ["your name <you@example.com>"]
edition = "2015"

[dependencies]
```

This will build your package in Rust 2015.
