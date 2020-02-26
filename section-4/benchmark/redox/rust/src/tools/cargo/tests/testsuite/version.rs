use cargo;
use cargo_test_support::project;

#[cargo_test]
fn simple() {
    let p = project().build();

    p.cargo("version")
        .with_stdout(&format!("{}\n", cargo::version()))
        .run();

    p.cargo("--version")
        .with_stdout(&format!("{}\n", cargo::version()))
        .run();
}

#[cargo_test]
#[cfg_attr(target_os = "windows", ignore)]
fn version_works_without_rustc() {
    let p = project().build();
    p.cargo("version").env("PATH", "").run();
}

#[cargo_test]
fn version_works_with_bad_config() {
    let p = project().file(".cargo/config", "this is not toml").build();
    p.cargo("version").run();
}

#[cargo_test]
fn version_works_with_bad_target_dir() {
    let p = project()
        .file(
            ".cargo/config",
            r#"
            [build]
            target-dir = 4
        "#,
        )
        .build();
    p.cargo("version").run();
}
