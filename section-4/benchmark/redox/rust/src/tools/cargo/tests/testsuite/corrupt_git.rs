use std::fs;
use std::path::{Path, PathBuf};

use cargo::util::paths as cargopaths;
use cargo_test_support::paths;
use cargo_test_support::{basic_manifest, git, project};

#[cargo_test]
fn deleting_database_files() {
    let project = project();
    let git_project = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "")
    });

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []

            [dependencies]
            bar = {{ git = '{}' }}
        "#,
                git_project.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    project.cargo("build").run();

    let mut files = Vec::new();
    find_files(&paths::home().join(".cargo/git/db"), &mut files);
    assert!(!files.is_empty());

    let log = "cargo::sources::git=trace";
    for file in files {
        if !file.exists() {
            continue;
        }
        println!("deleting {}", file.display());
        cargopaths::remove_file(&file).unwrap();
        project.cargo("build -v").env("CARGO_LOG", log).run();

        if !file.exists() {
            continue;
        }
        println!("truncating {}", file.display());
        make_writable(&file);
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_len(2)
            .unwrap();
        project.cargo("build -v").env("CARGO_LOG", log).run();
    }
}

#[cargo_test]
fn deleting_checkout_files() {
    let project = project();
    let git_project = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "")
    });

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []

            [dependencies]
            bar = {{ git = '{}' }}
        "#,
                git_project.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    project.cargo("build").run();

    let dir = paths::home()
        .join(".cargo/git/checkouts")
        // get the first entry in the checkouts dir for the package's location
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        // get the first child of that checkout dir for our checkout
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        // and throw on .git to corrupt things
        .join(".git");
    let mut files = Vec::new();
    find_files(&dir, &mut files);
    assert!(!files.is_empty());

    let log = "cargo::sources::git=trace";
    for file in files {
        if !file.exists() {
            continue;
        }
        println!("deleting {}", file.display());
        cargopaths::remove_file(&file).unwrap();
        project.cargo("build -v").env("CARGO_LOG", log).run();

        if !file.exists() {
            continue;
        }
        println!("truncating {}", file.display());
        make_writable(&file);
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_len(2)
            .unwrap();
        project.cargo("build -v").env("CARGO_LOG", log).run();
    }
}

fn make_writable(path: &Path) {
    let mut p = path.metadata().unwrap().permissions();
    p.set_readonly(false);
    fs::set_permissions(path, p).unwrap();
}

fn find_files(path: &Path, dst: &mut Vec<PathBuf>) {
    for e in path.read_dir().unwrap() {
        let e = e.unwrap();
        let path = e.path();
        if e.file_type().unwrap().is_dir() {
            find_files(&path, dst);
        } else {
            dst.push(path);
        }
    }
}
