use std::fs::File;
use std::io::prelude::*;

use cargo_test_support::paths::CargoPathExt;
use cargo_test_support::registry::Package;
use cargo_test_support::{basic_manifest, project, t};

#[cargo_test]
fn invalid1() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            bar = ["baz"]
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` includes `baz` which is neither a dependency nor another feature
",
        )
        .run();
}

#[cargo_test]
fn invalid2() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            bar = ["baz"]

            [dependencies.bar]
            path = "foo"
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Features and dependencies cannot have the same name: `bar`
",
        )
        .run();
}

#[cargo_test]
fn invalid3() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            bar = ["baz"]

            [dependencies.baz]
            path = "foo"
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` depends on `baz` which is not an optional dependency.
Consider adding `optional = true` to the dependency
",
        )
        .run();
}

#[cargo_test]
fn invalid4() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            features = ["bar"]
        "#,
        )
        .file("src/main.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr(
            "\
error: failed to select a version for `bar`.
    ... required by package `foo v0.0.1 ([..])`
versions that meet the requirements `*` are: 0.0.1

the package `foo` depends on `bar`, with features: `bar` but `bar` does not have these features.


failed to select a version for `bar` which could resolve this conflict",
        )
        .run();

    p.change_file("Cargo.toml", &basic_manifest("foo", "0.0.1"));

    p.cargo("build --features test")
        .with_status(101)
        .with_stderr("error: Package `foo v0.0.1 ([..])` does not have these features: `test`")
        .run();
}

#[cargo_test]
fn invalid5() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dev-dependencies.bar]
            path = "bar"
            optional = true
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Dev-dependencies are not allowed to be optional: `bar`
",
        )
        .run();
}

#[cargo_test]
fn invalid6() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            foo = ["bar/baz"]
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build --features foo")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `foo` requires a feature of `bar` which is not a dependency
",
        )
        .run();
}

#[cargo_test]
fn invalid7() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            foo = ["bar/baz"]
            bar = []
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build --features foo")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `foo` requires a feature of `bar` which is not a dependency
",
        )
        .run();
}

#[cargo_test]
fn invalid8() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            features = ["foo/bar"]
        "#,
        )
        .file("src/main.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("build --features foo")
        .with_status(101)
        .with_stderr("[ERROR] feature names may not contain slashes: `foo/bar`")
        .run();
}

#[cargo_test]
fn invalid9() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("build --features bar")
.with_stderr(
            "\
error: Package `foo v0.0.1 ([..])` does not have feature `bar`. It has a required dependency with that name, but only optional dependencies can be used as features.
",
        ).with_status(101).run();
}

#[cargo_test]
fn invalid10() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            features = ["baz"]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"
            authors = []

            [dependencies.baz]
            path = "baz"
        "#,
        )
        .file("bar/src/lib.rs", "")
        .file("bar/baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("bar/baz/src/lib.rs", "")
        .build();

    p.cargo("build").with_stderr("\
error: failed to select a version for `bar`.
    ... required by package `foo v0.0.1 ([..])`
versions that meet the requirements `*` are: 0.0.1

the package `foo` depends on `bar`, with features: `baz` but `bar` does not have these features.
 It has a required dependency with that name, but only optional dependencies can be used as features.


failed to select a version for `bar` which could resolve this conflict
").with_status(101)
        .run();
}

#[cargo_test]
fn no_transitive_dep_feature_requirement() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.derived]
            path = "derived"

            [features]
            default = ["derived/bar/qux"]
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            extern crate derived;
            fn main() { derived::test(); }
        "#,
        )
        .file(
            "derived/Cargo.toml",
            r#"
            [package]
            name = "derived"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "../bar"
        "#,
        )
        .file("derived/src/lib.rs", "extern crate bar; pub use bar::test;")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"
            authors = []

            [features]
            qux = []
        "#,
        )
        .file(
            "bar/src/lib.rs",
            r#"
            #[cfg(feature = "qux")]
            pub fn test() { print!("test"); }
        "#,
        )
        .build();
    p.cargo("build")
        .with_status(101)
        .with_stderr("[ERROR] feature names may not contain slashes: `bar/qux`")
        .run();
}

#[cargo_test]
fn no_feature_doesnt_build() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[cfg(feature = "bar")]
            extern crate bar;
            #[cfg(feature = "bar")]
            fn main() { bar::bar(); println!("bar") }
            #[cfg(not(feature = "bar"))]
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.process(&p.bin("foo")).with_stdout("").run();

    p.cargo("build --features bar")
        .with_stderr(
            "\
[COMPILING] bar v0.0.1 ([CWD]/bar)
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.process(&p.bin("foo")).with_stdout("bar\n").run();
}

#[cargo_test]
fn default_feature_pulled_in() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            default = ["bar"]

            [dependencies.bar]
            path = "bar"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[cfg(feature = "bar")]
            extern crate bar;
            #[cfg(feature = "bar")]
            fn main() { bar::bar(); println!("bar") }
            #[cfg(not(feature = "bar"))]
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] bar v0.0.1 ([CWD]/bar)
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.process(&p.bin("foo")).with_stdout("bar\n").run();

    p.cargo("build --no-default-features")
        .with_stderr(
            "\
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.process(&p.bin("foo")).with_stdout("").run();
}

#[cargo_test]
fn cyclic_feature() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            default = ["default"]
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr("[ERROR] cyclic feature dependency: feature `default` depends on itself")
        .run();
}

#[cargo_test]
fn cyclic_feature2() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            foo = ["bar"]
            bar = ["foo"]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").with_stdout("").run();
}

#[cargo_test]
fn groups_on_groups_on_groups() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            default = ["f1"]
            f1 = ["f2", "bar"]
            f2 = ["f3", "f4"]
            f3 = ["f5", "f6", "baz"]
            f4 = ["f5", "f7"]
            f5 = ["f6"]
            f6 = ["f7"]
            f7 = ["bar"]

            [dependencies.bar]
            path = "bar"
            optional = true

            [dependencies.baz]
            path = "baz"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate bar;
            #[allow(unused_extern_crates)]
            extern crate baz;
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("baz/src/lib.rs", "pub fn baz() {}")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn many_cli_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            optional = true

            [dependencies.baz]
            path = "baz"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate bar;
            #[allow(unused_extern_crates)]
            extern crate baz;
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("baz/src/lib.rs", "pub fn baz() {}")
        .build();

    p.cargo("build --features")
        .arg("bar baz")
        .with_stderr(
            "\
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn union_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.d1]
            path = "d1"
            features = ["f1"]
            [dependencies.d2]
            path = "d2"
            features = ["f2"]
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate d1;
            extern crate d2;
            fn main() {
                d2::f1();
                d2::f2();
            }
        "#,
        )
        .file(
            "d1/Cargo.toml",
            r#"
            [package]
            name = "d1"
            version = "0.0.1"
            authors = []

            [features]
            f1 = ["d2"]

            [dependencies.d2]
            path = "../d2"
            features = ["f1"]
            optional = true
        "#,
        )
        .file("d1/src/lib.rs", "")
        .file(
            "d2/Cargo.toml",
            r#"
            [package]
            name = "d2"
            version = "0.0.1"
            authors = []

            [features]
            f1 = []
            f2 = []
        "#,
        )
        .file(
            "d2/src/lib.rs",
            r#"
            #[cfg(feature = "f1")] pub fn f1() {}
            #[cfg(feature = "f2")] pub fn f2() {}
        "#,
        )
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] d2 v0.0.1 ([CWD]/d2)
[COMPILING] d1 v0.0.1 ([CWD]/d1)
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn many_features_no_rebuilds() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name    = "b"
            version = "0.1.0"
            authors = []

            [dependencies.a]
            path = "a"
            features = ["fall"]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "a/Cargo.toml",
            r#"
            [package]
            name    = "a"
            version = "0.1.0"
            authors = []

            [features]
            ftest  = []
            ftest2 = []
            fall   = ["ftest", "ftest2"]
        "#,
        )
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] a v0.1.0 ([CWD]/a)
[COMPILING] b v0.1.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.root().move_into_the_past();

    p.cargo("build -v")
        .with_stderr(
            "\
[FRESH] a v0.1.0 ([..]/a)
[FRESH] b v0.1.0 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

// Tests that all cmd lines work with `--features ""`
#[cargo_test]
fn empty_features() {
    let p = project().file("src/main.rs", "fn main() {}").build();

    p.cargo("build --features").arg("").run();
}

// Tests that all cmd lines work with `--features ""`
#[cargo_test]
fn transitive_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            foo = ["bar/baz"]

            [dependencies.bar]
            path = "bar"
        "#,
        )
        .file("src/main.rs", "extern crate bar; fn main() { bar::baz(); }")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"
            authors = []

            [features]
            baz = []
        "#,
        )
        .file(
            "bar/src/lib.rs",
            r#"#[cfg(feature = "baz")] pub fn baz() {}"#,
        )
        .build();

    p.cargo("build --features foo").run();
}

#[cargo_test]
fn everything_in_the_lockfile() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            f1 = ["d1/f1"]
            f2 = ["d2"]

            [dependencies.d1]
            path = "d1"
            [dependencies.d2]
            path = "d2"
            optional = true
            [dependencies.d3]
            path = "d3"
            optional = true
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "d1/Cargo.toml",
            r#"
            [package]
            name = "d1"
            version = "0.0.1"
            authors = []

            [features]
            f1 = []
        "#,
        )
        .file("d1/src/lib.rs", "")
        .file("d2/Cargo.toml", &basic_manifest("d2", "0.0.2"))
        .file("d2/src/lib.rs", "")
        .file(
            "d3/Cargo.toml",
            r#"
            [package]
            name = "d3"
            version = "0.0.3"
            authors = []

            [features]
            f3 = []
        "#,
        )
        .file("d3/src/lib.rs", "")
        .build();

    p.cargo("fetch").run();
    let loc = p.root().join("Cargo.lock");
    let mut lockfile = String::new();
    t!(t!(File::open(&loc)).read_to_string(&mut lockfile));
    assert!(
        lockfile.contains(r#"name = "d1""#),
        "d1 not found\n{}",
        lockfile
    );
    assert!(
        lockfile.contains(r#"name = "d2""#),
        "d2 not found\n{}",
        lockfile
    );
    assert!(
        lockfile.contains(r#"name = "d3""#),
        "d3 not found\n{}",
        lockfile
    );
}

#[cargo_test]
fn no_rebuild_when_frobbing_default_feature() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            authors = []

            [dependencies]
            a = { path = "a" }
            b = { path = "b" }
        "#,
        )
        .file("src/lib.rs", "")
        .file(
            "b/Cargo.toml",
            r#"
            [package]
            name = "b"
            version = "0.1.0"
            authors = []

            [dependencies]
            a = { path = "../a", features = ["f1"], default-features = false }
        "#,
        )
        .file("b/src/lib.rs", "")
        .file(
            "a/Cargo.toml",
            r#"
            [package]
            name = "a"
            version = "0.1.0"
            authors = []

            [features]
            default = ["f1"]
            f1 = []
        "#,
        )
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build").run();
    p.cargo("build").with_stdout("").run();
    p.cargo("build").with_stdout("").run();
}

#[cargo_test]
fn unions_work_with_no_default_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            authors = []

            [dependencies]
            a = { path = "a" }
            b = { path = "b" }
        "#,
        )
        .file("src/lib.rs", "extern crate a; pub fn foo() { a::a(); }")
        .file(
            "b/Cargo.toml",
            r#"
            [package]
            name = "b"
            version = "0.1.0"
            authors = []

            [dependencies]
            a = { path = "../a", features = [], default-features = false }
        "#,
        )
        .file("b/src/lib.rs", "")
        .file(
            "a/Cargo.toml",
            r#"
            [package]
            name = "a"
            version = "0.1.0"
            authors = []

            [features]
            default = ["f1"]
            f1 = []
        "#,
        )
        .file("a/src/lib.rs", r#"#[cfg(feature = "f1")] pub fn a() {}"#)
        .build();

    p.cargo("build").run();
    p.cargo("build").with_stdout("").run();
    p.cargo("build").with_stdout("").run();
}

#[cargo_test]
fn optional_and_dev_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name    = "test"
            version = "0.1.0"
            authors = []

            [dependencies]
            foo = { path = "foo", optional = true }
            [dev-dependencies]
            foo = { path = "foo" }
        "#,
        )
        .file("src/lib.rs", "")
        .file("foo/Cargo.toml", &basic_manifest("foo", "0.1.0"))
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[COMPILING] test v0.1.0 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn activating_feature_activates_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name    = "test"
            version = "0.1.0"
            authors = []

            [dependencies]
            foo = { path = "foo", optional = true }

            [features]
            a = ["foo/a"]
        "#,
        )
        .file(
            "src/lib.rs",
            "extern crate foo; pub fn bar() { foo::bar(); }",
        )
        .file(
            "foo/Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            authors = []

            [features]
            a = []
        "#,
        )
        .file("foo/src/lib.rs", r#"#[cfg(feature = "a")] pub fn bar() {}"#)
        .build();

    p.cargo("build --features a -v").run();
}

#[cargo_test]
fn dep_feature_in_cmd_line() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.derived]
            path = "derived"
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            extern crate derived;
            fn main() { derived::test(); }
        "#,
        )
        .file(
            "derived/Cargo.toml",
            r#"
            [package]
            name = "derived"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "../bar"

            [features]
            default = []
            derived-feat = ["bar/some-feat"]
        "#,
        )
        .file("derived/src/lib.rs", "extern crate bar; pub use bar::test;")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"
            authors = []

            [features]
            some-feat = []
        "#,
        )
        .file(
            "bar/src/lib.rs",
            r#"
            #[cfg(feature = "some-feat")]
            pub fn test() { print!("test"); }
        "#,
        )
        .build();

    // The foo project requires that feature "some-feat" in "bar" is enabled.
    // Building without any features enabled should fail:
    p.cargo("build")
        .with_status(101)
        .with_stderr_contains("[..]unresolved import `bar::test`")
        .run();

    // We should be able to enable the feature "derived-feat", which enables "some-feat",
    // on the command line. The feature is enabled, thus building should be successful:
    p.cargo("build --features derived/derived-feat").run();

    // Trying to enable features of transitive dependencies is an error
    p.cargo("build --features bar/some-feat")
        .with_status(101)
        .with_stderr("error: Package `foo v0.0.1 ([..])` does not have these features: `bar`")
        .run();

    // Hierarchical feature specification should still be disallowed
    p.cargo("build --features derived/bar/some-feat")
        .with_status(101)
        .with_stderr("[ERROR] feature names may not contain slashes: `bar/some-feat`")
        .run();
}

#[cargo_test]
fn all_features_flag_enables_all_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [features]
            foo = []
            bar = []

            [dependencies.baz]
            path = "baz"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[cfg(feature = "foo")]
            pub fn foo() {}

            #[cfg(feature = "bar")]
            pub fn bar() {
                extern crate baz;
                baz::baz();
            }

            fn main() {
                foo();
                bar();
            }
        "#,
        )
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("baz/src/lib.rs", "pub fn baz() {}")
        .build();

    p.cargo("build --all-features").run();
}

#[cargo_test]
fn many_cli_features_comma_delimited() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            optional = true

            [dependencies.baz]
            path = "baz"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate bar;
            #[allow(unused_extern_crates)]
            extern crate baz;
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("baz/src/lib.rs", "pub fn baz() {}")
        .build();

    p.cargo("build --features bar,baz")
        .with_stderr(
            "\
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn many_cli_features_comma_and_space_delimited() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            optional = true

            [dependencies.baz]
            path = "baz"
            optional = true

            [dependencies.bam]
            path = "bam"
            optional = true

            [dependencies.bap]
            path = "bap"
            optional = true
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate bar;
            #[allow(unused_extern_crates)]
            extern crate baz;
            #[allow(unused_extern_crates)]
            extern crate bam;
            #[allow(unused_extern_crates)]
            extern crate bap;
            fn main() {}
        "#,
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.0.1"))
        .file("baz/src/lib.rs", "pub fn baz() {}")
        .file("bam/Cargo.toml", &basic_manifest("bam", "0.0.1"))
        .file("bam/src/lib.rs", "pub fn bam() {}")
        .file("bap/Cargo.toml", &basic_manifest("bap", "0.0.1"))
        .file("bap/src/lib.rs", "pub fn bap() {}")
        .build();

    p.cargo("build --features")
        .arg("bar,baz bam bap")
        .with_stderr(
            "\
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] ba[..] v0.0.1 ([CWD]/ba[..])
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[cargo_test]
fn combining_features_and_package() {
    Package::new("dep", "1.0.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [workspace]
            members = ["bar"]

            [dependencies]
            dep = "1"
        "#,
        )
        .file("src/lib.rs", "")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"
            authors = []
            [features]
            main = []
        "#,
        )
        .file(
            "bar/src/main.rs",
            r#"
            #[cfg(feature = "main")]
            fn main() {}
        "#,
        )
        .build();

    p.cargo("build -Z package-features --workspace --features main")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr_contains("[ERROR] cannot specify features for more than one package")
        .run();

    p.cargo("build -Z package-features --package dep --features main")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr_contains("[ERROR] cannot specify features for packages outside of workspace")
        .run();
    p.cargo("build -Z package-features --package dep --all-features")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr_contains("[ERROR] cannot specify features for packages outside of workspace")
        .run();
    p.cargo("build -Z package-features --package dep --no-default-features")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr_contains("[ERROR] cannot specify features for packages outside of workspace")
        .run();

    p.cargo("build -Z package-features --workspace --all-features")
        .masquerade_as_nightly_cargo()
        .run();
    p.cargo("run -Z package-features --package bar --features main")
        .masquerade_as_nightly_cargo()
        .run();
    p.cargo("build -Z package-features --package dep")
        .masquerade_as_nightly_cargo()
        .run();
}

#[cargo_test]
fn namespaced_invalid_feature() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            bar = ["baz"]
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` includes `baz` which is not defined as a feature
",
        )
        .run();
}

#[cargo_test]
fn namespaced_invalid_dependency() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            bar = ["crate:baz"]
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` includes `crate:baz` which is not a known dependency
",
        )
        .run();
}

#[cargo_test]
fn namespaced_non_optional_dependency() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            bar = ["crate:baz"]

            [dependencies]
            baz = "0.1"
        "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` includes `crate:baz` which is not an optional dependency.
Consider adding `optional = true` to the dependency
",
        )
        .run();
}

#[cargo_test]
fn namespaced_implicit_feature() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            bar = ["baz"]

            [dependencies]
            baz = { version = "0.1", optional = true }
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").masquerade_as_nightly_cargo().run();
}

#[cargo_test]
fn namespaced_shadowed_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            baz = []

            [dependencies]
            baz = { version = "0.1", optional = true }
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").masquerade_as_nightly_cargo().with_status(101).with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `baz` includes the optional dependency of the same name, but this is left implicit in the features included by this feature.
Consider adding `crate:baz` to this feature's requirements.
",
        )
        .run();
}

#[cargo_test]
fn namespaced_shadowed_non_optional() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            baz = []

            [dependencies]
            baz = "0.1"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").masquerade_as_nightly_cargo().with_status(101).with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `baz` includes the dependency of the same name, but this is left implicit in the features included by this feature.
Additionally, the dependency must be marked as optional to be included in the feature definition.
Consider adding `crate:baz` to this feature's requirements and marking the dependency as `optional = true`
",
        )
        .run();
}

#[cargo_test]
fn namespaced_implicit_non_optional() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            bar = ["baz"]

            [dependencies]
            baz = "0.1"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").masquerade_as_nightly_cargo().with_status(101).with_stderr(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  Feature `bar` includes `baz` which is not defined as a feature.
A non-optional dependency of the same name is defined; consider adding `optional = true` to its definition
",
        ).run(
    );
}

#[cargo_test]
fn namespaced_same_name() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["namespaced-features"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            namespaced-features = true

            [features]
            baz = ["crate:baz"]

            [dependencies]
            baz = { version = "0.1", optional = true }
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").masquerade_as_nightly_cargo().run();
}

#[cargo_test]
fn only_dep_is_optional() {
    Package::new("bar", "0.1.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [features]
                foo = ['bar']

                [dependencies]
                bar = { version = "0.1", optional = true }

                [dev-dependencies]
                bar = "0.1"
            "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").run();
}

#[cargo_test]
fn all_features_all_crates() {
    Package::new("bar", "0.1.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [workspace]
                members = ['bar']
            "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "bar/Cargo.toml",
            r#"
                [project]
                name = "bar"
                version = "0.0.1"
                authors = []

                [features]
                foo = []
            "#,
        )
        .file("bar/src/main.rs", "#[cfg(feature = \"foo\")] fn main() {}")
        .build();

    p.cargo("build --all-features --workspace").run();
}

#[cargo_test]
fn feature_off_dylib() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [workspace]
            members = ["bar"]

            [package]
            name = "foo"
            version = "0.0.1"

            [lib]
            crate-type = ["dylib"]

            [features]
            f1 = []
        "#,
        )
        .file(
            "src/lib.rs",
            r#"
            pub fn hello() -> &'static str {
                if cfg!(feature = "f1") {
                    "f1"
                } else {
                    "no f1"
                }
            }
        "#,
        )
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.0.1"

            [dependencies]
            foo = { path = ".." }
        "#,
        )
        .file(
            "bar/src/main.rs",
            r#"
            extern crate foo;

            fn main() {
                assert_eq!(foo::hello(), "no f1");
            }
        "#,
        )
        .build();

    // Build the dylib with `f1` feature.
    p.cargo("build --features f1").run();
    // Check that building without `f1` uses a dylib without `f1`.
    p.cargo("run -p bar").run();
}

#[cargo_test]
fn warn_if_default_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            optional = true

            [features]
            default-features = ["bar"]
         "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .build();

    p.cargo("build")
        .with_stderr(
            r#"
[WARNING] `default-features = [".."]` was found in [features]. Did you mean to use `default = [".."]`?
[COMPILING] foo v0.0.1 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
            "#.trim(),
        ).run();
}

#[cargo_test]
fn no_feature_for_non_optional_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                bar = { path = "bar" }
             "#,
        )
        .file(
            "src/main.rs",
            r#"
                #[cfg(not(feature = "bar"))]
                fn main() {
                }
            "#,
        )
        .file(
            "bar/Cargo.toml",
            r#"
                [project]
                name = "bar"
                version = "0.0.1"
                authors = []

                [features]
                a = []
             "#,
        )
        .file("bar/src/lib.rs", "pub fn bar() {}")
        .build();

    p.cargo("build --features bar/a").run();
}

#[cargo_test]
fn features_option_given_twice() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [features]
                a = []
                b = []
             "#,
        )
        .file(
            "src/main.rs",
            r#"
                #[cfg(all(feature = "a", feature = "b"))]
                fn main() {}
            "#,
        )
        .build();

    p.cargo("build --features a --features b").run();
}

#[cargo_test]
fn multi_multi_features() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [features]
                a = []
                b = []
                c = []
            "#,
        )
        .file(
            "src/main.rs",
            r#"
               #[cfg(all(feature = "a", feature = "b", feature = "c"))]
               fn main() {}
            "#,
        )
        .build();

    p.cargo("build --features a --features").arg("b c").run();
}

#[cargo_test]
fn cli_parse_ok() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [project]
                name = "foo"
                version = "0.0.1"
                authors = []

                [features]
                a = []
            "#,
        )
        .file(
            "src/main.rs",
            r#"
               #[cfg(feature = "a")]
               fn main() {
                    assert_eq!(std::env::args().nth(1).unwrap(), "b");
               }
            "#,
        )
        .build();

    p.cargo("run --features a b").run();
}

#[cargo_test]
fn virtual_ws_flags() {
    // Reject features flags in the root of a virtual workspace.
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [workspace]
                members = ["a"]
            "#,
        )
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.1.0"

                [features]
                f1 = []
            "#,
        )
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build --features=f1")
        .with_stderr("[ERROR] --features is not allowed in the root of a virtual workspace")
        .with_status(101)
        .run();

    p.cargo("build --no-default-features")
        .with_stderr(
            "[ERROR] --no-default-features is not allowed in the root of a virtual workspace",
        )
        .with_status(101)
        .run();

    // It's OK if cwd is in a member.
    p.cargo("check --features=f1 -v")
        .cwd("a")
        .with_stderr(
            "\
[CHECKING] a [..]
[RUNNING] `rustc --crate-name a a/src/lib.rs [..]--cfg [..]feature[..]f1[..]
[FINISHED] dev [..]
",
        )
        .run();

    p.cargo("clean").run();

    // And -Zpackage-features is OK because it is designed to support this.
    p.cargo("check --features=f1 -p a -Z package-features -v")
        .masquerade_as_nightly_cargo()
        .with_stderr(
            "\
[CHECKING] a [..]
[RUNNING] `rustc --crate-name a a/src/lib.rs [..]--cfg [..]feature[..]f1[..]
[FINISHED] dev [..]
",
        )
        .run();
}

#[cargo_test]
fn all_features_virtual_ws() {
    // What happens with `--all-features` in the root of a virtual workspace.
    // Some of this behavior is a little strange (member dependencies also
    // have all features enabled, one might expect `f4` to be disabled).
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [workspace]
                members = ["a", "b"]
            "#,
        )
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.1.0"
                edition = "2018"

                [dependencies]
                b = {path="../b", optional=true}

                [features]
                default = ["f1"]
                f1 = []
                f2 = []
            "#,
        )
        .file(
            "a/src/main.rs",
            r#"
                fn main() {
                    if cfg!(feature="f1") {
                        println!("f1");
                    }
                    if cfg!(feature="f2") {
                        println!("f2");
                    }
                    #[cfg(feature="b")]
                    b::f();
                }
            "#,
        )
        .file(
            "b/Cargo.toml",
            r#"
                [package]
                name = "b"
                version = "0.1.0"

                [features]
                default = ["f3"]
                f3 = []
                f4 = []
            "#,
        )
        .file(
            "b/src/lib.rs",
            r#"
                pub fn f() {
                    if cfg!(feature="f3") {
                        println!("f3");
                    }
                    if cfg!(feature="f4") {
                        println!("f4");
                    }
                }
            "#,
        )
        .build();

    p.cargo("run").with_stdout("f1\n").run();
    p.cargo("run --all-features")
        .with_stdout("f1\nf2\nf3\nf4\n")
        .run();
    // In `a`, it behaves differently. :(
    p.cargo("run --all-features")
        .cwd("a")
        .with_stdout("f1\nf2\nf3\n")
        .run();
}
