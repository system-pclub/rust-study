use std::fs::File;
use std::io::prelude::*;

use cargo_test_support::registry::Package;
use cargo_test_support::{basic_manifest, project};

#[cargo_test]
fn minor_update_two_places() {
    Package::new("log", "0.1.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1"
                foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();
    Package::new("log", "0.1.1").publish();

    File::create(p.root().join("foo/Cargo.toml"))
        .unwrap()
        .write_all(
            br#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1.1"
            "#,
        )
        .unwrap();

    p.cargo("build").run();
}

#[cargo_test]
fn transitive_minor_update() {
    Package::new("log", "0.1.0").publish();
    Package::new("serde", "0.1.0").dep("log", "0.1").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
                log = "0.1"
                foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();

    Package::new("log", "0.1.1").publish();
    Package::new("serde", "0.1.1").dep("log", "0.1.1").publish();

    // Note that `serde` isn't actually updated here! The default behavior for
    // `update` right now is to as conservatively as possible attempt to satisfy
    // an update. In this case we previously locked the dependency graph to `log
    // 0.1.0`, but nothing on the command line says we're allowed to update
    // that. As a result the update of `serde` here shouldn't update to `serde
    // 0.1.1` as that would also force an update to `log 0.1.1`.
    //
    // Also note that this is probably counterintuitive and weird. We may wish
    // to change this one day.
    p.cargo("update -p serde")
        .with_stderr(
            "\
[UPDATING] `[..]` index
",
        )
        .run();
}

#[cargo_test]
fn conservative() {
    Package::new("log", "0.1.0").publish();
    Package::new("serde", "0.1.0").dep("log", "0.1").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
                log = "0.1"
                foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();

    Package::new("log", "0.1.1").publish();
    Package::new("serde", "0.1.1").dep("log", "0.1").publish();

    p.cargo("update -p serde")
        .with_stderr(
            "\
[UPDATING] `[..]` index
[UPDATING] serde v0.1.0 -> v0.1.1
",
        )
        .run();
}

#[cargo_test]
fn update_via_new_dep() {
    Package::new("log", "0.1.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1"
                # foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();
    Package::new("log", "0.1.1").publish();

    p.uncomment_root_manifest();
    p.cargo("build").env("CARGO_LOG", "cargo=trace").run();
}

#[cargo_test]
fn update_via_new_member() {
    Package::new("log", "0.1.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [workspace]
                # members = [ "foo" ]

                [dependencies]
                log = "0.1"
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();
    Package::new("log", "0.1.1").publish();

    p.uncomment_root_manifest();
    p.cargo("build").run();
}

#[cargo_test]
fn add_dep_deep_new_requirement() {
    Package::new("log", "0.1.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                log = "0.1"
                # bar = "0.1"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build").run();

    Package::new("log", "0.1.1").publish();
    Package::new("bar", "0.1.0").dep("log", "0.1.1").publish();

    p.uncomment_root_manifest();
    p.cargo("build").run();
}

#[cargo_test]
fn everything_real_deep() {
    Package::new("log", "0.1.0").publish();
    Package::new("foo", "0.1.0").dep("log", "0.1").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                foo = "0.1"
                # bar = "0.1"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build").run();

    Package::new("log", "0.1.1").publish();
    Package::new("bar", "0.1.0").dep("log", "0.1.1").publish();

    p.uncomment_root_manifest();
    p.cargo("build").run();
}

#[cargo_test]
fn change_package_version() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "a-foo"
                version = "0.2.0-alpha"
                authors = []

                [dependencies]
                bar = { path = "bar", version = "0.2.0-alpha" }
            "#,
        )
        .file("src/lib.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.2.0-alpha"))
        .file("bar/src/lib.rs", "")
        .file(
            "Cargo.lock",
            r#"
                [[package]]
                name = "foo"
                version = "0.2.0"
                dependencies = ["bar 0.2.0"]

                [[package]]
                name = "bar"
                version = "0.2.0"
            "#,
        )
        .build();

    p.cargo("build").run();
}

#[cargo_test]
fn update_precise() {
    Package::new("log", "0.1.0").publish();
    Package::new("serde", "0.1.0").publish();
    Package::new("serde", "0.2.1").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.2"
                foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();

    Package::new("serde", "0.2.0").publish();

    p.cargo("update -p serde:0.2.1 --precise 0.2.0")
        .with_stderr(
            "\
[UPDATING] `[..]` index
[UPDATING] serde v0.2.1 -> v0.2.0
",
        )
        .run();
}

// cargo update should respect its arguments even without a lockfile.
// See issue "Running cargo update without a Cargo.lock ignores arguments"
// at <https://github.com/rust-lang/cargo/issues/6872>.
#[cargo_test]
fn update_precise_first_run() {
    Package::new("serde", "0.1.0").publish();
    Package::new("serde", "0.2.0").publish();
    Package::new("serde", "0.2.1").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"

                [dependencies]
                serde = "0.2"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("update -p serde --precise 0.2.0")
        .with_stderr(
            "\
[UPDATING] `[..]` index
[UPDATING] serde v0.2.1 -> v0.2.0
",
        )
        .run();

    // Assert `cargo metadata` shows serde 0.2.0
    p.cargo("metadata")
        .with_json(
            r#"{
  "packages": [
    {
      "authors": [],
      "categories": [],
      "dependencies": [],
      "description": null,
      "edition": "2015",
      "features": {},
      "id": "serde 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)",
      "keywords": [],
      "license": null,
      "license_file": null,
      "links": null,
      "manifest_path": "[..]/home/.cargo/registry/src/-[..]/serde-0.2.0/Cargo.toml",
      "metadata": null,
      "publish": null,
      "name": "serde",
      "readme": null,
      "repository": null,
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "targets": [
        {
          "crate_types": [
            "lib"
          ],
          "doctest": true,
          "edition": "2015",
          "kind": [
            "lib"
          ],
          "name": "serde",
          "src_path": "[..]/home/.cargo/registry/src/-[..]/serde-0.2.0/src/lib.rs"
        }
      ],
      "version": "0.2.0"
    },
    {
      "authors": [],
      "categories": [],
      "dependencies": [
        {
          "features": [],
          "kind": null,
          "name": "serde",
          "optional": false,
          "registry": null,
          "rename": null,
          "req": "^0.2",
          "source": "registry+https://github.com/rust-lang/crates.io-index",
          "target": null,
          "uses_default_features": true
        }
      ],
      "description": null,
      "edition": "2015",
      "features": {},
      "id": "bar 0.0.1 (path+file://[..]/foo)",
      "keywords": [],
      "license": null,
      "license_file": null,
      "links": null,
      "manifest_path": "[..]/foo/Cargo.toml",
      "metadata": null,
      "publish": null,
      "name": "bar",
      "readme": null,
      "repository": null,
      "source": null,
      "targets": [
        {
          "crate_types": [
            "lib"
          ],
          "doctest": true,
          "edition": "2015",
          "kind": [
            "lib"
          ],
          "name": "bar",
          "src_path": "[..]/foo/src/lib.rs"
        }
      ],
      "version": "0.0.1"
    }
  ],
  "resolve": {
    "nodes": [
      {
        "dependencies": [
          "serde 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)"
        ],
        "deps": [
          {
            "name": "serde",
            "pkg": "serde 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)"
          }
        ],
        "features": [],
        "id": "bar 0.0.1 (path+file://[..]/foo)"
      },
      {
        "dependencies": [],
        "deps": [],
        "features": [],
        "id": "serde 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)"
      }
    ],
    "root": "bar 0.0.1 (path+file://[..]/foo)"
  },
  "target_directory": "[..]/foo/target",
  "version": 1,
  "workspace_members": [
    "bar 0.0.1 (path+file://[..]/foo)"
  ],
  "workspace_root": "[..]/foo"
}"#,
        )
        .run();

    p.cargo("update -p serde --precise 0.2.0")
        .with_stderr(
            "\
[UPDATING] `[..]` index
",
        )
        .run();
}

#[cargo_test]
fn preserve_top_comment() {
    let p = project().file("src/lib.rs", "").build();

    p.cargo("update").run();

    let lockfile = p.read_lockfile();
    assert!(lockfile.starts_with("# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\n"));

    let mut lines = lockfile.lines().collect::<Vec<_>>();
    lines.insert(2, "# some other comment");
    let mut lockfile = lines.join("\n");
    lockfile.push_str("\n"); // .lines/.join loses the last newline
    println!("saving Cargo.lock contents:\n{}", lockfile);

    p.change_file("Cargo.lock", &lockfile);

    p.cargo("update").run();

    let lockfile2 = p.read_lockfile();
    println!("loaded Cargo.lock contents:\n{}", lockfile2);

    assert_eq!(lockfile, lockfile2);
}

#[cargo_test]
fn dry_run_update() {
    Package::new("log", "0.1.0").publish();
    Package::new("serde", "0.1.0").dep("log", "0.1").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
                log = "0.1"
                foo = { path = "foo" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "foo/Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                authors = []

                [dependencies]
                serde = "0.1"
            "#,
        )
        .file("foo/src/lib.rs", "")
        .build();

    p.cargo("build").run();
    let old_lockfile = p.read_file("Cargo.lock");

    Package::new("log", "0.1.1").publish();
    Package::new("serde", "0.1.1").dep("log", "0.1").publish();

    p.cargo("update -p serde --dry-run")
        .with_stderr(
            "\
[UPDATING] `[..]` index
[UPDATING] serde v0.1.0 -> v0.1.1
[WARNING] not updating lockfile due to dry run
",
        )
        .run();
    let new_lockfile = p.read_file("Cargo.lock");
    assert_eq!(old_lockfile, new_lockfile)
}
