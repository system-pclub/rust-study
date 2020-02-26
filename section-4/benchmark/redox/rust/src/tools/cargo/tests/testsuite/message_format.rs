use cargo_test_support::{basic_manifest, project};

#[cargo_test]
fn cannot_specify_two() {
    let p = project()
        .file("Cargo.toml", &basic_manifest("foo", "0.1.0"))
        .file("src/main.rs", "fn main() {}")
        .build();

    let formats = ["human", "json", "short"];

    let two_kinds = "error: cannot specify two kinds of `message-format` arguments\n";
    for a in formats.iter() {
        for b in formats.iter() {
            p.cargo(&format!("build --message-format {},{}", a, b))
                .with_status(101)
                .with_stderr(two_kinds)
                .run();
        }
    }
}

#[cargo_test]
fn double_json_works() {
    let p = project()
        .file("Cargo.toml", &basic_manifest("foo", "0.1.0"))
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build --message-format json,json-render-diagnostics")
        .run();
    p.cargo("build --message-format json,json-diagnostic-short")
        .run();
    p.cargo("build --message-format json,json-diagnostic-rendered-ansi")
        .run();
    p.cargo("build --message-format json --message-format json-diagnostic-rendered-ansi")
        .run();
    p.cargo("build --message-format json-diagnostic-rendered-ansi")
        .run();
    p.cargo("build --message-format json-diagnostic-short,json-diagnostic-rendered-ansi")
        .run();
}

#[cargo_test]
fn cargo_renders() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = 'foo'
                version = '0.1.0'

                [dependencies]
                bar = { path = 'bar' }
            "#,
        )
        .file("src/main.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.1.0"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("build --message-format json-render-diagnostics")
        .with_status(101)
        .with_stdout("{\"reason\":\"compiler-artifact\",[..]")
        .with_stderr_contains(
            "\
[COMPILING] bar [..]
[COMPILING] foo [..]
error[..]`main`[..]
",
        )
        .run();
}

#[cargo_test]
fn cargo_renders_short() {
    let p = project()
        .file("Cargo.toml", &basic_manifest("foo", "0.1.0"))
        .file("src/main.rs", "")
        .build();

    p.cargo("build --message-format json-render-diagnostics,json-diagnostic-short")
        .with_status(101)
        .with_stderr_contains(
            "\
[COMPILING] foo [..]
error[..]`main`[..]
",
        )
        .with_stderr_does_not_contain("note:")
        .run();
}

#[cargo_test]
fn cargo_renders_ansi() {
    let p = project()
        .file("Cargo.toml", &basic_manifest("foo", "0.1.0"))
        .file("src/main.rs", "")
        .build();

    p.cargo("build --message-format json-diagnostic-rendered-ansi")
        .with_status(101)
        .with_stdout_contains("[..]\\u001b[38;5;9merror[..]")
        .run();
}
