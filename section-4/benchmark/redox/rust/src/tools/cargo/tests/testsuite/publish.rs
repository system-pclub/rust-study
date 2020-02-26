use std::fs::{self, File};
use std::io::prelude::*;

use cargo_test_support::git::{self, repo};
use cargo_test_support::paths;
use cargo_test_support::registry::{self, registry_path, registry_url, Package};
use cargo_test_support::{basic_manifest, project, publish};

const CLEAN_FOO_JSON: &str = r#"
    {
        "authors": [],
        "badges": {},
        "categories": [],
        "deps": [],
        "description": "foo",
        "documentation": "foo",
        "features": {},
        "homepage": "foo",
        "keywords": [],
        "license": "MIT",
        "license_file": null,
        "links": null,
        "name": "foo",
        "readme": null,
        "readme_file": null,
        "repository": "foo",
        "vers": "0.0.1"
    }
"#;

fn validate_upload_foo() {
    publish::validate_upload(
        r#"
        {
          "authors": [],
          "badges": {},
          "categories": [],
          "deps": [],
          "description": "foo",
          "documentation": null,
          "features": {},
          "homepage": null,
          "keywords": [],
          "license": "MIT",
          "license_file": null,
          "links": null,
          "name": "foo",
          "readme": null,
          "readme_file": null,
          "repository": null,
          "vers": "0.0.1"
          }
        "#,
        "foo-0.0.1.crate",
        &["Cargo.lock", "Cargo.toml", "Cargo.toml.orig", "src/main.rs"],
    );
}

fn validate_upload_foo_clean() {
    publish::validate_upload(
        CLEAN_FOO_JSON,
        "foo-0.0.1.crate",
        &[
            "Cargo.lock",
            "Cargo.toml",
            "Cargo.toml.orig",
            "src/main.rs",
            ".cargo_vcs_info.json",
        ],
    );
}

#[cargo_test]
fn simple() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .with_stderr(&format!(
            "\
[UPDATING] `{reg}` index
[WARNING] manifest has no documentation, [..]
See [..]
[PACKAGING] foo v0.0.1 ([CWD])
[UPLOADING] foo v0.0.1 ([CWD])
",
            reg = registry::registry_path().to_str().unwrap()
        ))
        .run();

    validate_upload_foo();
}

#[cargo_test]
fn old_token_location() {
    // Check that the `token` key works at the root instead of under a
    // `[registry]` table.
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    let credentials = paths::home().join(".cargo/credentials");
    fs::remove_file(&credentials).unwrap();

    // Verify can't publish without a token.
    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr_contains("[ERROR] no upload token found, please run `cargo login`")
        .run();

    File::create(&credentials)
        .unwrap()
        .write_all(br#"token = "api-token""#)
        .unwrap();

    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .with_stderr(&format!(
            "\
[UPDATING] `{reg}` index
[WARNING] manifest has no documentation, [..]
See [..]
[PACKAGING] foo v0.0.1 ([CWD])
[UPLOADING] foo v0.0.1 ([CWD])
",
            reg = registry_path().to_str().unwrap()
        ))
        .run();

    validate_upload_foo();
}

// TODO: Deprecated
// remove once it has been decided --host can be removed
#[cargo_test]
fn simple_with_host() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --no-verify --host")
        .arg(registry_url().to_string())
        .with_stderr(&format!(
            "\
[WARNING] The flag '--host' is no longer valid.

Previous versions of Cargo accepted this flag, but it is being
deprecated. The flag is being renamed to 'index', as the flag
wants the location of the index. Please use '--index' instead.

This will soon become a hard error, so it's either recommended
to update to a fixed version or contact the upstream maintainer
about this warning.
[UPDATING] `{reg}` index
[WARNING] manifest has no documentation, [..]
See [..]
[PACKAGING] foo v0.0.1 ([CWD])
[UPLOADING] foo v0.0.1 ([CWD])
",
            reg = registry_path().to_str().unwrap()
        ))
        .run();

    validate_upload_foo();
}

// TODO: Deprecated
// remove once it has been decided --host can be removed
#[cargo_test]
fn simple_with_index_and_host() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .arg("--host")
        .arg(registry_url().to_string())
        .with_stderr(&format!(
            "\
[WARNING] The flag '--host' is no longer valid.

Previous versions of Cargo accepted this flag, but it is being
deprecated. The flag is being renamed to 'index', as the flag
wants the location of the index. Please use '--index' instead.

This will soon become a hard error, so it's either recommended
to update to a fixed version or contact the upstream maintainer
about this warning.
[UPDATING] `{reg}` index
[WARNING] manifest has no documentation, [..]
See [..]
[PACKAGING] foo v0.0.1 ([CWD])
[UPLOADING] foo v0.0.1 ([CWD])
",
            reg = registry_path().to_str().unwrap()
        ))
        .run();

    validate_upload_foo();
}

#[cargo_test]
fn git_deps() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [dependencies.foo]
            git = "git://path/to/nowhere"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish -v --no-verify --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr(
            "\
[UPDATING] [..] index
[ERROR] all dependencies must have a version specified when publishing.
dependency `foo` does not specify a version
Note: The published dependency will use the version from crates.io,
the `git` specification will be removed from the dependency declaration.
",
        )
        .run();
}

#[cargo_test]
fn path_dependency_no_version() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [dependencies.bar]
            path = "bar"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr(
            "\
[UPDATING] [..] index
[ERROR] all dependencies must have a version specified when publishing.
dependency `bar` does not specify a version
Note: The published dependency will use the version from crates.io,
the `path` specification will be removed from the dependency declaration.
",
        )
        .run();
}

#[cargo_test]
fn unpublishable_crate() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish = false
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr(
            "\
[ERROR] `foo` cannot be published.
The registry `crates-io` is not listed in the `publish` value in Cargo.toml.
",
        )
        .run();
}

#[cargo_test]
fn dont_publish_dirty() {
    registry::init();
    let p = project().file("bar", "").build();

    let _ = git::repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr(
            "\
[UPDATING] `[..]` index
error: 1 files in the working directory contain changes that were not yet \
committed into git:

bar

to proceed despite this and include the uncommited changes, pass the `--allow-dirty` flag
",
        )
        .run();
}

#[cargo_test]
fn publish_clean() {
    registry::init();

    let p = project().build();

    let _ = repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .run();

    validate_upload_foo_clean();
}

#[cargo_test]
fn publish_in_sub_repo() {
    registry::init();

    let p = project().no_manifest().file("baz", "").build();

    let _ = repo(&paths::root().join("foo"))
        .file(
            "bar/Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .file("bar/src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish")
        .cwd("bar")
        .arg("--index")
        .arg(registry_url().to_string())
        .run();

    validate_upload_foo_clean();
}

#[cargo_test]
fn publish_when_ignored() {
    registry::init();

    let p = project().file("baz", "").build();

    let _ = repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(".gitignore", "baz")
        .build();

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .run();

    publish::validate_upload(
        CLEAN_FOO_JSON,
        "foo-0.0.1.crate",
        &[
            "Cargo.lock",
            "Cargo.toml",
            "Cargo.toml.orig",
            "src/main.rs",
            ".gitignore",
            ".cargo_vcs_info.json",
        ],
    );
}

#[cargo_test]
fn ignore_when_crate_ignored() {
    registry::init();

    let p = project().no_manifest().file("bar/baz", "").build();

    let _ = repo(&paths::root().join("foo"))
        .file(".gitignore", "bar")
        .nocommit_file(
            "bar/Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .nocommit_file("bar/src/main.rs", "fn main() {}");
    p.cargo("publish")
        .cwd("bar")
        .arg("--index")
        .arg(registry_url().to_string())
        .run();

    publish::validate_upload(
        CLEAN_FOO_JSON,
        "foo-0.0.1.crate",
        &[
            "Cargo.lock",
            "Cargo.toml",
            "Cargo.toml.orig",
            "src/main.rs",
            "baz",
        ],
    );
}

#[cargo_test]
fn new_crate_rejected() {
    registry::init();

    let p = project().file("baz", "").build();

    let _ = repo(&paths::root().join("foo"))
        .nocommit_file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        )
        .nocommit_file("src/main.rs", "fn main() {}");
    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .with_status(101)
        .with_stderr_contains(
            "[ERROR] 3 files in the working directory contain \
             changes that were not yet committed into git:",
        )
        .run();
}

#[cargo_test]
fn dry_run() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --dry-run --index")
        .arg(registry_url().to_string())
        .with_stderr(
            "\
[UPDATING] `[..]` index
[WARNING] manifest has no documentation, [..]
See [..]
[PACKAGING] foo v0.0.1 ([CWD])
[VERIFYING] foo v0.0.1 ([CWD])
[COMPILING] foo v0.0.1 [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
[UPLOADING] foo v0.0.1 ([CWD])
[WARNING] aborting upload due to dry run
",
        )
        .run();

    // Ensure the API request wasn't actually made
    assert!(registry::api_path().join("api/v1/crates").exists());
    assert!(!registry::api_path().join("api/v1/crates/new").exists());
}

#[cargo_test]
fn registry_not_in_publish_list() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish = [
                "test"
            ]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish")
        .arg("--registry")
        .arg("alternative")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] `foo` cannot be published.
The registry `alternative` is not listed in the `publish` value in Cargo.toml.
",
        )
        .run();
}

#[cargo_test]
fn publish_empty_list() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish = []
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --registry alternative")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] `foo` cannot be published.
The registry `alternative` is not listed in the `publish` value in Cargo.toml.
",
        )
        .run();
}

#[cargo_test]
fn publish_allowed_registry() {
    registry::init();

    let p = project().build();

    let _ = repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
            publish = ["alternative"]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --registry alternative").run();

    publish::validate_alt_upload(
        CLEAN_FOO_JSON,
        "foo-0.0.1.crate",
        &[
            "Cargo.lock",
            "Cargo.toml",
            "Cargo.toml.orig",
            "src/main.rs",
            ".cargo_vcs_info.json",
        ],
    );
}

#[cargo_test]
fn block_publish_no_registry() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish = []
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --registry alternative")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] `foo` cannot be published.
The registry `alternative` is not listed in the `publish` value in Cargo.toml.
",
        )
        .run();
}

#[cargo_test]
fn publish_with_crates_io_explicit() {
    // Explicitly setting `crates-io` in the publish list.
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish = ["crates-io"]
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("publish --registry alternative")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] `foo` cannot be published.
The registry `alternative` is not listed in the `publish` value in Cargo.toml.
",
        )
        .run();

    p.cargo("publish").run();
}

#[cargo_test]
fn publish_with_select_features() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [features]
            required = []
            optional = []
        "#,
        )
        .file(
            "src/main.rs",
            "#[cfg(not(feature = \"required\"))]
             compile_error!(\"This crate requires `required` feature!\");
             fn main() {}",
        )
        .build();

    p.cargo("publish --features required --index")
        .arg(registry_url().to_string())
        .with_stderr_contains("[UPLOADING] foo v0.0.1 ([CWD])")
        .run();
}

#[cargo_test]
fn publish_with_all_features() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [features]
            required = []
            optional = []
        "#,
        )
        .file(
            "src/main.rs",
            "#[cfg(not(feature = \"required\"))]
             compile_error!(\"This crate requires `required` feature!\");
             fn main() {}",
        )
        .build();

    p.cargo("publish --all-features --index")
        .arg(registry_url().to_string())
        .with_stderr_contains("[UPLOADING] foo v0.0.1 ([CWD])")
        .run();
}

#[cargo_test]
fn publish_with_no_default_features() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [features]
            default = ["required"]
            required = []
        "#,
        )
        .file(
            "src/main.rs",
            "#[cfg(not(feature = \"required\"))]
             compile_error!(\"This crate requires `required` feature!\");
             fn main() {}",
        )
        .build();

    p.cargo("publish --no-default-features --index")
        .arg(registry_url().to_string())
        .with_stderr_contains("error: This crate requires `required` feature!")
        .with_status(101)
        .run();
}

#[cargo_test]
fn publish_with_patch() {
    Package::new("bar", "1.0.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            [dependencies]
            bar = "1.0"
            [patch.crates-io]
            bar = { path = "bar" }
        "#,
        )
        .file(
            "src/main.rs",
            "extern crate bar;
             fn main() {
                 bar::newfunc();
             }",
        )
        .file("bar/Cargo.toml", &basic_manifest("bar", "1.0.0"))
        .file("bar/src/lib.rs", "pub fn newfunc() {}")
        .build();

    // Check that it works with the patched crate.
    p.cargo("build").run();

    // Check that verify fails with patched crate which has new functionality.
    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .with_stderr_contains("[..]newfunc[..]")
        .with_status(101)
        .run();

    // Remove the usage of new functionality and try again.
    p.change_file("src/main.rs", "extern crate bar; pub fn main() {}");

    p.cargo("publish --index")
        .arg(registry_url().to_string())
        .run();

    // Note, use of `registry` in the deps here is an artifact that this
    // publishes to a fake, local registry that is pretending to be crates.io.
    // Normal publishes would set it to null.
    publish::validate_upload(
        r#"
        {
          "authors": [],
          "badges": {},
          "categories": [],
          "deps": [
            {
              "default_features": true,
              "features": [],
              "kind": "normal",
              "name": "bar",
              "optional": false,
              "registry": "https://github.com/rust-lang/crates.io-index",
              "target": null,
              "version_req": "^1.0"
            }
          ],
          "description": "foo",
          "documentation": null,
          "features": {},
          "homepage": null,
          "keywords": [],
          "license": "MIT",
          "license_file": null,
          "links": null,
          "name": "foo",
          "readme": null,
          "readme_file": null,
          "repository": null,
          "vers": "0.0.1"
          }
        "#,
        "foo-0.0.1.crate",
        &["Cargo.lock", "Cargo.toml", "Cargo.toml.orig", "src/main.rs"],
    );
}

#[cargo_test]
fn publish_checks_for_token_before_verify() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    let credentials = paths::home().join(".cargo/credentials");
    fs::remove_file(&credentials).unwrap();

    // Assert upload token error before the package is verified
    p.cargo("publish")
        .with_status(101)
        .with_stderr_contains("[ERROR] no upload token found, please run `cargo login`")
        .with_stderr_does_not_contain("[VERIFYING] foo v0.0.1 ([CWD])")
        .run();

    // Assert package verified successfully on dry run
    p.cargo("publish --dry-run")
        .with_status(0)
        .with_stderr_contains("[VERIFYING] foo v0.0.1 ([CWD])")
        .run();
}

#[cargo_test]
fn publish_with_bad_source() {
    let p = project()
        .file(
            ".cargo/config",
            r#"
            [source.crates-io]
            replace-with = 'local-registry'

            [source.local-registry]
            local-registry = 'registry'
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("publish")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] registry `[..]/foo/registry` does not support API commands.
Check for a source-replacement in .cargo/config.
",
        )
        .run();

    p.change_file(
        ".cargo/config",
        r#"
        [source.crates-io]
        replace-with = "vendored-sources"

        [source.vendored-sources]
        directory = "vendor"
        "#,
    );

    p.cargo("publish")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] dir [..]/foo/vendor does not support API commands.
Check for a source-replacement in .cargo/config.
",
        )
        .run();
}

#[cargo_test]
fn publish_git_with_version() {
    // A dependency with both `git` and `version`.
    Package::new("dep1", "1.0.1")
        .file("src/lib.rs", "pub fn f() -> i32 {1}")
        .publish();

    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep1", "1.0.0"))
            .file("src/lib.rs", "pub fn f() -> i32 {2}")
    });

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [package]
                name = "foo"
                version = "0.1.0"
                authors = []
                edition = "2018"
                license = "MIT"
                description = "foo"

                [dependencies]
                dep1 = {{version = "1.0", git="{}"}}
                "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            r#"
            pub fn main() {
                println!("{}", dep1::f());
            }
            "#,
        )
        .build();

    p.cargo("run").with_stdout("2").run();
    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .run();

    publish::validate_upload_with_contents(
        r#"
        {
          "authors": [],
          "badges": {},
          "categories": [],
          "deps": [
            {
              "default_features": true,
              "features": [],
              "kind": "normal",
              "name": "dep1",
              "optional": false,
              "registry": "https://github.com/rust-lang/crates.io-index",
              "target": null,
              "version_req": "^1.0"
            }
          ],
          "description": "foo",
          "documentation": null,
          "features": {},
          "homepage": null,
          "keywords": [],
          "license": "MIT",
          "license_file": null,
          "links": null,
          "name": "foo",
          "readme": null,
          "readme_file": null,
          "repository": null,
          "vers": "0.1.0"
          }
        "#,
        "foo-0.1.0.crate",
        &["Cargo.lock", "Cargo.toml", "Cargo.toml.orig", "src/main.rs"],
        &[
            (
                "Cargo.toml",
                // Check that only `version` is included in Cargo.toml.
                "[..]\n\
                 [dependencies.dep1]\n\
                 version = \"1.0\"\n\
                 ",
            ),
            (
                "Cargo.lock",
                // The important check here is that it is 1.0.1 in the registry.
                "[..]\n\
                 [[package]]\n\
                 name = \"foo\"\n\
                 version = \"0.1.0\"\n\
                 dependencies = [\n\
                 \x20\"dep1 1.0.1 (registry+https://github.com/rust-lang/crates.io-index)\",\n\
                 ]\n\
                 [..]",
            ),
        ],
    );
}

#[cargo_test]
fn publish_dev_dep_no_version() {
    registry::init();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            authors = []
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"

            [dev-dependencies]
            bar = { path = "bar" }
            "#,
        )
        .file("src/lib.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("publish --no-verify --index")
        .arg(registry_url().to_string())
        .with_stderr(
            "\
[UPDATING] [..]
[PACKAGING] foo v0.1.0 [..]
[UPLOADING] foo v0.1.0 [..]
",
        )
        .run();

    publish::validate_upload_with_contents(
        r#"
        {
          "authors": [],
          "badges": {},
          "categories": [],
          "deps": [],
          "description": "foo",
          "documentation": "foo",
          "features": {},
          "homepage": "foo",
          "keywords": [],
          "license": "MIT",
          "license_file": null,
          "links": null,
          "name": "foo",
          "readme": null,
          "readme_file": null,
          "repository": "foo",
          "vers": "0.1.0"
        }
        "#,
        "foo-0.1.0.crate",
        &["Cargo.toml", "Cargo.toml.orig", "src/lib.rs"],
        &[(
            "Cargo.toml",
            r#"[..]
[package]
name = "foo"
version = "0.1.0"
authors = []
description = "foo"
homepage = "foo"
documentation = "foo"
license = "MIT"
repository = "foo"

[dev-dependencies]
"#,
        )],
    );
}
