use crate::paths;
use std::env::consts::EXE_SUFFIX;
use std::path::{Path, PathBuf};

/// Used by `cargo install` tests to assert an executable binary
/// has been installed. Example usage:
///
///     assert_has_installed_exe(cargo_home(), "foo");
pub fn assert_has_installed_exe<P: AsRef<Path>>(path: P, name: &'static str) {
    assert!(check_has_installed_exe(path, name));
}

pub fn assert_has_not_installed_exe<P: AsRef<Path>>(path: P, name: &'static str) {
    assert!(!check_has_installed_exe(path, name));
}

fn check_has_installed_exe<P: AsRef<Path>>(path: P, name: &'static str) -> bool {
    path.as_ref().join("bin").join(exe(name)).is_file()
}

pub fn cargo_home() -> PathBuf {
    paths::home().join(".cargo")
}

pub fn exe(name: &str) -> String {
    format!("{}{}", name, EXE_SUFFIX)
}
