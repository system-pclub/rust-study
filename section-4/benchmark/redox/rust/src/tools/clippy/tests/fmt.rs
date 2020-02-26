use std::process::Command;

#[test]
fn fmt() {
    if option_env!("RUSTC_TEST_SUITE").is_some() {
        return;
    }

    // Skip this test if rustup nightly is unavailable
    let rustup_output = Command::new("rustup")
        .args(&["component", "list", "--toolchain", "nightly"])
        .output()
        .unwrap();
    assert!(rustup_output.status.success());
    let component_output = String::from_utf8_lossy(&rustup_output.stdout);
    if !component_output.contains("rustfmt") {
        return;
    }

    let root_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dev_dir = root_dir.join("clippy_dev");
    let output = Command::new("cargo")
        .current_dir(dev_dir)
        .args(&["+nightly", "run", "--", "fmt", "--check"])
        .output()
        .unwrap();

    println!("status: {}", output.status);
    println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "Formatting check failed. Run `./util/dev fmt` to update formatting."
    );
}
