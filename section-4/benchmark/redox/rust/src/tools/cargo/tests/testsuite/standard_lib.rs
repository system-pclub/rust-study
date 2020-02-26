use cargo_test_support::registry::{Dependency, Package};
use cargo_test_support::ProjectBuilder;
use cargo_test_support::{is_nightly, paths, project, rustc_host, Execs};
use std::path::PathBuf;

struct Setup {
    rustc_wrapper: PathBuf,
    real_sysroot: String,
}

fn setup() -> Option<Setup> {
    if !is_nightly() {
        // -Zbuild-std is nightly
        // We don't want these tests to run on rust-lang/rust.
        return None;
    }

    // Our mock sysroot requires a few packages from crates.io, so make sure
    // they're "published" to crates.io. Also edit their code a bit to make sure
    // that they have access to our custom crates with custom apis.
    Package::new("registry-dep-using-core", "1.0.0")
        .file(
            "src/lib.rs",
            "
                #![no_std]

                #[cfg(feature = \"mockbuild\")]
                pub fn custom_api() {
                }

                #[cfg(not(feature = \"mockbuild\"))]
                pub fn non_sysroot_api() {
                    core::custom_api();
                }
            ",
        )
        .add_dep(Dependency::new("rustc-std-workspace-core", "*").optional(true))
        .feature("mockbuild", &["rustc-std-workspace-core"])
        .publish();
    Package::new("registry-dep-using-alloc", "1.0.0")
        .file(
            "src/lib.rs",
            "
                #![no_std]

                extern crate alloc;

                #[cfg(feature = \"mockbuild\")]
                pub fn custom_api() {
                }

                #[cfg(not(feature = \"mockbuild\"))]
                pub fn non_sysroot_api() {
                    core::custom_api();
                    alloc::custom_api();
                }
            ",
        )
        .add_dep(Dependency::new("rustc-std-workspace-core", "*").optional(true))
        .add_dep(Dependency::new("rustc-std-workspace-alloc", "*").optional(true))
        .feature(
            "mockbuild",
            &["rustc-std-workspace-core", "rustc-std-workspace-alloc"],
        )
        .publish();
    Package::new("registry-dep-using-std", "1.0.0")
        .file(
            "src/lib.rs",
            "
                #[cfg(feature = \"mockbuild\")]
                pub fn custom_api() {
                }

                #[cfg(not(feature = \"mockbuild\"))]
                pub fn non_sysroot_api() {
                    std::custom_api();
                }
            ",
        )
        .add_dep(Dependency::new("rustc-std-workspace-std", "*").optional(true))
        .feature("mockbuild", &["rustc-std-workspace-std"])
        .publish();

    let p = ProjectBuilder::new(paths::root().join("rustc-wrapper"))
        .file(
            "src/main.rs",
            r#"
                use std::process::Command;
                use std::env;
                fn main() {
                    let mut args = env::args().skip(1).collect::<Vec<_>>();

                    let is_sysroot_crate = env::var_os("RUSTC_BOOTSTRAP").is_some();
                    if is_sysroot_crate {
                        let arg = args.iter().position(|a| a == "--sysroot").unwrap();
                        args[arg + 1] = env::var("REAL_SYSROOT").unwrap();
                    }

                    let ret = Command::new(&args[0]).args(&args[1..]).status().unwrap();
                    std::process::exit(ret.code().unwrap_or(1));
                }
            "#,
        )
        .build();
    p.cargo("build").run();

    Some(Setup {
        rustc_wrapper: p.bin("foo"),
        real_sysroot: paths::sysroot(),
    })
}

fn enable_build_std(e: &mut Execs, setup: &Setup, arg: Option<&str>) {
    // First up, force Cargo to use our "mock sysroot" which mimics what
    // libstd looks like upstream.
    let root = paths::root();
    let root = root
        .parent() // chop off test name
        .unwrap()
        .parent() // chop off `citN`
        .unwrap()
        .parent() // chop off `target`
        .unwrap()
        .join("tests/testsuite/mock-std");
    e.env("__CARGO_TESTS_ONLY_SRC_ROOT", &root);

    // Actually enable `-Zbuild-std` for now
    let arg = match arg {
        Some(s) => format!("-Zbuild-std={}", s),
        None => "-Zbuild-std".to_string(),
    };
    e.arg(arg);
    e.masquerade_as_nightly_cargo();

    // We do various shenanigans to ensure our "mock sysroot" actually links
    // with the real sysroot, so we don't have to actually recompile std for
    // each test. Perform all that logic here, namely:
    //
    // * RUSTC_WRAPPER - uses our shim executable built above to control rustc
    // * REAL_SYSROOT - used by the shim executable to swap out to the real
    //   sysroot temporarily for some compilations
    // * RUST{,DOC}FLAGS - an extra `-L` argument to ensure we can always load
    //   crates from the sysroot, but only indirectly through other crates.
    e.env("RUSTC_WRAPPER", &setup.rustc_wrapper);
    e.env("REAL_SYSROOT", &setup.real_sysroot);
    let libdir = format!("/lib/rustlib/{}/lib", rustc_host());
    e.env(
        "RUSTFLAGS",
        format!("-Ldependency={}{}", setup.real_sysroot, libdir),
    );
    e.env(
        "RUSTDOCFLAGS",
        format!("-Ldependency={}{}", setup.real_sysroot, libdir),
    );
}

// Helper methods used in the tests below
trait BuildStd: Sized {
    fn build_std(&mut self, setup: &Setup) -> &mut Self;
    fn build_std_arg(&mut self, setup: &Setup, arg: &str) -> &mut Self;
    fn target_host(&mut self) -> &mut Self;
}

impl BuildStd for Execs {
    fn build_std(&mut self, setup: &Setup) -> &mut Self {
        enable_build_std(self, setup, None);
        self
    }

    fn build_std_arg(&mut self, setup: &Setup, arg: &str) -> &mut Self {
        enable_build_std(self, setup, Some(arg));
        self
    }

    fn target_host(&mut self) -> &mut Self {
        self.arg("--target").arg(rustc_host());
        self
    }
}

#[cargo_test]
fn basic() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };

    let p = project()
        .file(
            "src/main.rs",
            "
                fn main() {
                    std::custom_api();
                    foo::f();
                }

                #[test]
                fn smoke_bin_unit() {
                    std::custom_api();
                    foo::f();
                }
            ",
        )
        .file(
            "src/lib.rs",
            "
                extern crate alloc;
                extern crate proc_macro;

                /// ```
                /// foo::f();
                /// ```
                pub fn f() {
                    core::custom_api();
                    std::custom_api();
                    alloc::custom_api();
                    proc_macro::custom_api();
                }

                #[test]
                fn smoke_lib_unit() {
                    std::custom_api();
                    f();
                }
            ",
        )
        .file(
            "tests/smoke.rs",
            "
                #[test]
                fn smoke_integration() {
                    std::custom_api();
                    foo::f();
                }
            ",
        )
        .build();

    p.cargo("check").build_std(&setup).target_host().run();
    p.cargo("build").build_std(&setup).target_host().run();
    p.cargo("run").build_std(&setup).target_host().run();
    p.cargo("test").build_std(&setup).target_host().run();
}

#[cargo_test]
fn simple_lib_std() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project().file("src/lib.rs", "").build();
    p.cargo("build -v")
        .build_std(&setup)
        .target_host()
        .with_stderr_contains("[RUNNING] `[..]--crate-name std [..]`")
        .run();
    // Check freshness.
    p.change_file("src/lib.rs", " ");
    p.cargo("build -v")
        .build_std(&setup)
        .target_host()
        .with_stderr_contains("[FRESH] std[..]")
        .run();
}

#[cargo_test]
fn simple_bin_std() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project().file("src/main.rs", "fn main() {}").build();
    p.cargo("run -v").build_std(&setup).target_host().run();
}

#[cargo_test]
fn lib_nostd() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                #![no_std]
                pub fn foo() {
                    assert_eq!(core::u8::MIN, 0);
                }
            "#,
        )
        .build();
    p.cargo("build -v --lib")
        .build_std_arg(&setup, "core")
        .target_host()
        .with_stderr_does_not_contain("[..]libstd[..]")
        .run();
}

#[cargo_test]
fn check_core() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file("src/lib.rs", "#![no_std] fn unused_fn() {}")
        .build();

    p.cargo("check -v")
        .build_std_arg(&setup, "core")
        .target_host()
        .with_stderr_contains("[WARNING] [..]unused_fn[..]`")
        .run();
}

#[cargo_test]
fn depend_same_as_std() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };

    let p = project()
        .file(
            "src/lib.rs",
            r#"
                pub fn f() {
                    registry_dep_using_core::non_sysroot_api();
                    registry_dep_using_alloc::non_sysroot_api();
                    registry_dep_using_std::non_sysroot_api();
                }
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2018"

                [dependencies]
                registry-dep-using-core = "1.0"
                registry-dep-using-alloc = "1.0"
                registry-dep-using-std = "1.0"
            "#,
        )
        .build();

    p.cargo("build -v").build_std(&setup).target_host().run();
}

#[cargo_test]
fn test() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                #[cfg(test)]
                mod tests {
                    #[test]
                    fn it_works() {
                        assert_eq!(2 + 2, 4);
                    }
                }
            "#,
        )
        .build();

    p.cargo("test -v")
        .build_std(&setup)
        .target_host()
        .with_stdout_contains("test tests::it_works ... ok")
        .run();
}

#[cargo_test]
fn target_proc_macro() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                extern crate proc_macro;
                pub fn f() {
                    let _ts = proc_macro::TokenStream::new();
                }
            "#,
        )
        .build();

    p.cargo("build -v").build_std(&setup).target_host().run();
}

#[cargo_test]
fn bench() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                #![feature(test)]
                extern crate test;

                #[bench]
                fn b1(b: &mut test::Bencher) {
                    b.iter(|| ())
                }
            "#,
        )
        .build();

    p.cargo("bench -v").build_std(&setup).target_host().run();
}

#[cargo_test]
fn doc() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                /// Doc
                pub fn f() -> Result<(), ()> {Ok(())}
            "#,
        )
        .build();

    p.cargo("doc -v").build_std(&setup).target_host().run();
}

#[cargo_test]
fn check_std() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            "
                extern crate core;
                extern crate alloc;
                extern crate proc_macro;
                pub fn f() {}
            ",
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "tests/t1.rs",
            r#"
                #[test]
                fn t1() {
                    assert_eq!(1, 2);
                }
            "#,
        )
        .build();

    p.cargo("check -v --all-targets")
        .build_std(&setup)
        .target_host()
        .run();
    p.cargo("check -v --all-targets --profile=test")
        .build_std(&setup)
        .target_host()
        .run();
}

#[cargo_test]
fn doctest() {
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                /// Doc
                /// ```
                /// std::custom_api();
                /// ```
                pub fn f() {}
            "#,
        )
        .build();

    p.cargo("test --doc -v")
        .build_std(&setup)
        .with_stdout_contains("test src/lib.rs - f [..] ... ok")
        .target_host()
        .run();
}

#[cargo_test]
fn no_implicit_alloc() {
    // Demonstrate that alloc is not implicitly in scope.
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                pub fn f() {
                    let _: Vec<i32> = alloc::vec::Vec::new();
                }
            "#,
        )
        .build();

    p.cargo("build -v")
        .build_std(&setup)
        .target_host()
        .with_stderr_contains("[..]use of undeclared [..]`alloc`")
        .with_status(101)
        .run();
}

#[cargo_test]
fn macro_expanded_shadow() {
    // This tests a bug caused by the previous use of `--extern` to directly
    // load sysroot crates. This necessitated the switch to `--sysroot` to
    // retain existing behavior. See
    // https://github.com/rust-lang/wg-cargo-std-aware/issues/40 for more
    // detail.
    let setup = match setup() {
        Some(s) => s,
        None => return,
    };
    let p = project()
        .file(
            "src/lib.rs",
            r#"
                macro_rules! a {
                    () => (extern crate std as alloc;)
                }
                a!();
            "#,
        )
        .build();

    p.cargo("build -v").build_std(&setup).target_host().run();
}
