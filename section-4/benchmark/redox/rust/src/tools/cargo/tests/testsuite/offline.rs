use cargo_test_support::{basic_manifest, git, main_file, path2url, project, registry::Package};
use std::fs;

#[cargo_test]
fn offline_unused_target_dep() {
    // --offline with a target dependency that is not used and not downloaded.
    Package::new("unused_dep", "1.0.0").publish();
    Package::new("used_dep", "1.0.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.1.0"
            [dependencies]
            used_dep = "1.0"
            [target.'cfg(unused)'.dependencies]
            unused_dep = "1.0"
            "#,
        )
        .file("src/lib.rs", "")
        .build();
    // Do a build that downloads only what is necessary.
    p.cargo("build")
        .with_stderr_contains("[DOWNLOADED] used_dep [..]")
        .with_stderr_does_not_contain("[DOWNLOADED] unused_dep [..]")
        .run();
    p.cargo("clean").run();
    // Build offline, make sure it works.
    p.cargo("build --offline").run();
}

#[cargo_test]
fn offline_missing_optional() {
    Package::new("opt_dep", "1.0.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.1.0"
            [dependencies]
            opt_dep = { version = "1.0", optional = true }
            "#,
        )
        .file("src/lib.rs", "")
        .build();
    // Do a build that downloads only what is necessary.
    p.cargo("build")
        .with_stderr_does_not_contain("[DOWNLOADED] opt_dep [..]")
        .run();
    p.cargo("clean").run();
    // Build offline, make sure it works.
    p.cargo("build --offline").run();
    p.cargo("build --offline --features=opt_dep")
        .with_stderr(
            "\
[ERROR] failed to download `opt_dep v1.0.0`

Caused by:
  can't make HTTP request in the offline mode
",
        )
        .with_status(101)
        .run();
}

#[cargo_test]
fn cargo_compile_path_with_offline() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies.bar]
            path = "bar"
            "#,
        )
        .file("src/lib.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("build --offline").run();
}

#[cargo_test]
fn cargo_compile_with_downloaded_dependency_with_offline() {
    Package::new("present_dep", "1.2.3")
        .file("Cargo.toml", &basic_manifest("present_dep", "1.2.3"))
        .file("src/lib.rs", "")
        .publish();

    // make package downloaded
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.1.0"

            [dependencies]
            present_dep = "1.2.3"
            "#,
        )
        .file("src/lib.rs", "")
        .build();
    p.cargo("build").run();

    let p2 = project()
        .at("bar")
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "bar"
            version = "0.1.0"

            [dependencies]
            present_dep = "1.2.3"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p2.cargo("build --offline")
        .with_stderr(
            "\
[COMPILING] present_dep v1.2.3
[COMPILING] bar v0.1.0 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]",
        )
        .run();
}

#[cargo_test]
fn cargo_compile_offline_not_try_update() {
    let p = project()
        .at("bar")
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "bar"
            version = "0.1.0"

            [dependencies]
            not_cached_dep = "1.2.5"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build --offline")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to load source for a dependency on `not_cached_dep`

Caused by:
  Unable to update registry `https://github.com/rust-lang/crates.io-index`

Caused by:
  unable to fetch registry `https://github.com/rust-lang/crates.io-index` in offline mode
Try running without the offline flag, or try running `cargo fetch` within your \
project directory before going offline.
",
        )
        .run();

    p.change_file(".cargo/config", "net.offline = true");
    p.cargo("build")
        .with_status(101)
        .with_stderr_contains("[..]Unable to update registry[..]")
        .run();
}

#[cargo_test]
fn compile_offline_without_maxvers_cached() {
    Package::new("present_dep", "1.2.1").publish();
    Package::new("present_dep", "1.2.2").publish();

    Package::new("present_dep", "1.2.3")
        .file("Cargo.toml", &basic_manifest("present_dep", "1.2.3"))
        .file(
            "src/lib.rs",
            r#"pub fn get_version()->&'static str {"1.2.3"}"#,
        )
        .publish();

    Package::new("present_dep", "1.2.5")
        .file("Cargo.toml", &basic_manifest("present_dep", "1.2.5"))
        .file("src/lib.rs", r#"pub fn get_version(){"1.2.5"}"#)
        .publish();

    // make package cached
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.1.0"

            [dependencies]
            present_dep = "=1.2.3"
            "#,
        )
        .file("src/lib.rs", "")
        .build();
    p.cargo("build").run();

    let p2 = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.1.0"

            [dependencies]
            present_dep = "1.2"
            "#,
        )
        .file(
            "src/main.rs",
            "\
extern crate present_dep;
fn main(){
    println!(\"{}\", present_dep::get_version());
}",
        )
        .build();

    p2.cargo("run --offline")
        .with_stderr(
            "\
[COMPILING] present_dep v1.2.3
[COMPILING] foo v0.1.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
     Running `[..]`",
        )
        .with_stdout("1.2.3")
        .run();
}

#[cargo_test]
fn cargo_compile_forbird_git_httpsrepo_offline() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"

            [project]
            name = "foo"
            version = "0.5.0"
            authors = ["chabapok@example.com"]

            [dependencies.dep1]
            git = 'https://github.com/some_user/dep1.git'
            "#,
        )
        .file("src/main.rs", "")
        .build();

    p.cargo("build --offline").with_status(101).with_stderr("\
error: failed to load source for a dependency on `dep1`

Caused by:
  Unable to update https://github.com/some_user/dep1.git

Caused by:
  can't checkout from 'https://github.com/some_user/dep1.git': you are in the offline mode (--offline)").run();
}

#[cargo_test]
fn compile_offline_while_transitive_dep_not_cached() {
    let baz = Package::new("baz", "1.0.0");
    let baz_path = baz.archive_dst();
    baz.publish();

    let baz_content = fs::read(&baz_path).unwrap();
    // Truncate the file to simulate a download failure.
    fs::write(&baz_path, &[]).unwrap();

    Package::new("bar", "0.1.0").dep("baz", "1.0.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"

            [dependencies]
            bar = "0.1.0"
            "#,
        )
        .file("src/main.rs", "fn main(){}")
        .build();

    // simulate download bar, but fail to download baz
    p.cargo("build")
        .with_status(101)
        .with_stderr_contains("[..]failed to verify the checksum of `baz[..]")
        .run();

    // Restore the file contents.
    fs::write(&baz_path, &baz_content).unwrap();

    p.cargo("build --offline")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] failed to download `baz v1.0.0`

Caused by:
  can't make HTTP request in the offline mode
",
        )
        .run();
}

#[cargo_test]
fn update_offline() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            bar = "*"
            "#,
        )
        .file("src/main.rs", "fn main() {}")
        .build();
    p.cargo("update --offline")
        .with_status(101)
        .with_stderr("error: you can't update in the offline mode[..]")
        .run();
}

#[cargo_test]
fn cargo_compile_offline_with_cached_git_dep() {
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
            .file(
                "src/lib.rs",
                r#"
                pub static COOL_STR:&str = "cached git repo rev1";
                "#,
            )
    });

    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let rev1 = repo.revparse_single("HEAD").unwrap().id();

    // Commit the changes and make sure we trigger a recompile
    git_project.change_file(
        "src/lib.rs",
        r#"pub static COOL_STR:&str = "cached git repo rev2";"#,
    );
    git::add(&repo);
    let rev2 = git::commit(&repo);

    // cache to registry rev1 and rev2
    let prj = project()
        .at("cache_git_dep")
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [project]
                name = "cache_git_dep"
                version = "0.5.0"

                [dependencies.dep1]
                git = '{}'
                rev = "{}"
                "#,
                git_project.url(),
                rev1
            ),
        )
        .file("src/main.rs", "fn main(){}")
        .build();
    prj.cargo("build").run();

    prj.change_file(
        "Cargo.toml",
        &format!(
            r#"
            [project]
            name = "cache_git_dep"
            version = "0.5.0"

            [dependencies.dep1]
            git = '{}'
            rev = "{}"
            "#,
            git_project.url(),
            rev2
        ),
    );
    prj.cargo("build").run();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [project]
                name = "foo"
                version = "0.5.0"

                [dependencies.dep1]
                git = '{}'
                "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            &main_file(r#""hello from {}", dep1::COOL_STR"#, &["dep1"]),
        )
        .build();

    let git_root = git_project.root();

    p.cargo("build --offline")
        .with_stderr(format!(
            "\
[COMPILING] dep1 v0.5.0 ({}#[..])
[COMPILING] foo v0.5.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]",
            path2url(git_root),
        ))
        .run();

    assert!(p.bin("foo").is_file());

    p.process(&p.bin("foo"))
        .with_stdout("hello from cached git repo rev2\n")
        .run();

    p.change_file(
        "Cargo.toml",
        &format!(
            r#"
            [project]
            name = "foo"
            version = "0.5.0"

            [dependencies.dep1]
            git = '{}'
            rev = "{}"
            "#,
            git_project.url(),
            rev1
        ),
    );

    p.cargo("build --offline").run();
    p.process(&p.bin("foo"))
        .with_stdout("hello from cached git repo rev1\n")
        .run();
}

#[cargo_test]
fn offline_resolve_optional_fail() {
    // Example where resolve fails offline.
    //
    // This happens if at least 1 version of an optional dependency is
    // available, but none of them satisfy the requirements. The current logic
    // that handles this is `RegistryIndex::query_inner`, and it doesn't know
    // if the package being queried is an optional one. This is not ideal, it
    // would be best if it just ignored optional (unselected) dependencies.
    Package::new("dep", "1.0.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"

            [dependencies]
            dep = { version = "1.0", optional = true }
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("fetch").run();

    // Change dep to 2.0.
    p.change_file(
        "Cargo.toml",
        r#"
            [package]
            name = "foo"
            version = "0.1.0"

            [dependencies]
            dep = { version = "2.0", optional = true }
            "#,
    );

    p.cargo("build --offline")
        .with_status(101)
        .with_stderr("\
[ERROR] failed to select a version for the requirement `dep = \"^2.0\"`
  candidate versions found which didn't match: 1.0.0
  location searched: `[..]` index (which is replacing registry `https://github.com/rust-lang/crates.io-index`)
required by package `foo v0.1.0 ([..]/foo)`
perhaps a crate was updated and forgotten to be re-vendored?
As a reminder, you're using offline mode (--offline) which can sometimes cause \
surprising resolution failures, if this error is too confusing you may wish to \
retry without the offline flag.
")
        .run();
}
