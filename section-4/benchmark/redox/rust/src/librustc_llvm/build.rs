use std::process::Command;
use std::env;
use std::path::{PathBuf, Path};

use build_helper::output;

fn detect_llvm_link() -> (&'static str, &'static str) {
    // Force the link mode we want, preferring static by default, but
    // possibly overridden by `configure --enable-llvm-link-shared`.
    if env::var_os("LLVM_LINK_SHARED").is_some() {
        ("dylib", "--link-shared")
    } else {
        ("static", "--link-static")
    }
}

fn main() {
    if env::var_os("RUST_CHECK").is_some() {
        // If we're just running `check`, there's no need for LLVM to be built.
        println!("cargo:rerun-if-env-changed=RUST_CHECK");
        return;
    }

    build_helper::restore_library_path();

    let target = env::var("TARGET").expect("TARGET was not set");
    let llvm_config = env::var_os("LLVM_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if let Some(dir) = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
                let to_test = dir.parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join(&target)
                    .join("llvm/bin/llvm-config");
                if Command::new(&to_test).output().is_ok() {
                    return to_test;
                }
            }
            PathBuf::from("llvm-config")
        });

    println!("cargo:rerun-if-changed={}", llvm_config.display());
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG");

    // Test whether we're cross-compiling LLVM. This is a pretty rare case
    // currently where we're producing an LLVM for a different platform than
    // what this build script is currently running on.
    //
    // In that case, there's no guarantee that we can actually run the target,
    // so the build system works around this by giving us the LLVM_CONFIG for
    // the host platform. This only really works if the host LLVM and target
    // LLVM are compiled the same way, but for us that's typically the case.
    //
    // We *want* detect this cross compiling situation by asking llvm-config
    // what its host-target is. If that's not the TARGET, then we're cross
    // compiling. Unfortunately `llvm-config` seems either be buggy, or we're
    // misconfiguring it, because the `i686-pc-windows-gnu` build of LLVM will
    // report itself with a `--host-target` of `x86_64-pc-windows-gnu`. This
    // tricks us into thinking we're doing a cross build when we aren't, so
    // havoc ensues.
    //
    // In any case, if we're cross compiling, this generally just means that we
    // can't trust all the output of llvm-config because it might be targeted
    // for the host rather than the target. As a result a bunch of blocks below
    // are gated on `if !is_crossed`
    let target = env::var("TARGET").expect("TARGET was not set");
    let host = env::var("HOST").expect("HOST was not set");
    let is_crossed = target != host;

    let mut optional_components =
        vec!["x86", "arm", "aarch64", "amdgpu", "mips", "powerpc",
             "systemz", "jsbackend", "webassembly", "msp430", "sparc", "nvptx",
             "hexagon"];

    let mut version_cmd = Command::new(&llvm_config);
    version_cmd.arg("--version");
    let version_output = output(&mut version_cmd);
    let mut parts = version_output.split('.').take(2)
        .filter_map(|s| s.parse::<u32>().ok());
    let (major, _minor) =
        if let (Some(major), Some(minor)) = (parts.next(), parts.next()) {
            (major, minor)
        } else {
            (6, 0)
        };

    if major > 6 {
        optional_components.push("riscv");
    }

    let required_components = &["ipo",
                                "bitreader",
                                "bitwriter",
                                "linker",
                                "asmparser",
                                "lto",
                                "instrumentation"];

    let components = output(Command::new(&llvm_config).arg("--components"));
    let mut components = components.split_whitespace().collect::<Vec<_>>();
    components.retain(|c| optional_components.contains(c) || required_components.contains(c));

    for component in required_components {
        if !components.contains(component) {
            panic!("require llvm component {} but wasn't found", component);
        }
    }

    for component in components.iter() {
        println!("cargo:rustc-cfg=llvm_component=\"{}\"", component);
    }

    if major >= 9 {
        println!("cargo:rustc-cfg=llvm_has_msp430_asm_parser");
    }

    // Link in our own LLVM shims, compiled with the same flags as LLVM
    let mut cmd = Command::new(&llvm_config);
    cmd.arg("--cxxflags");
    let cxxflags = output(&mut cmd);
    let mut cfg = cc::Build::new();
    cfg.warnings(false);
    for flag in cxxflags.split_whitespace() {
        // Ignore flags like `-m64` when we're doing a cross build
        if is_crossed && flag.starts_with("-m") {
            continue;
        }

        if flag.starts_with("-flto") {
            continue;
        }

        // -Wdate-time is not supported by the netbsd cross compiler
        if is_crossed && target.contains("netbsd") && flag.contains("date-time") {
            continue;
        }

        cfg.flag(flag);
    }

    for component in &components {
        let mut flag = String::from("LLVM_COMPONENT_");
        flag.push_str(&component.to_uppercase());
        cfg.define(&flag, None);
    }

    println!("cargo:rerun-if-changed-env=LLVM_RUSTLLVM");
    if env::var_os("LLVM_RUSTLLVM").is_some() {
        cfg.define("LLVM_RUSTLLVM", None);
    }

    if env::var_os("LLVM_NDEBUG").is_some() {
        cfg.define("NDEBUG", None);
    }

    build_helper::rerun_if_changed_anything_in_dir(Path::new("../rustllvm"));
    cfg.file("../rustllvm/PassWrapper.cpp")
       .file("../rustllvm/RustWrapper.cpp")
       .file("../rustllvm/ArchiveWrapper.cpp")
       .file("../rustllvm/Linker.cpp")
       .cpp(true)
       .cpp_link_stdlib(None) // we handle this below
       .compile("rustllvm");

    let (llvm_kind, llvm_link_arg) = detect_llvm_link();

    // Link in all LLVM libraries, if we're using the "wrong" llvm-config then
    // we don't pick up system libs because unfortunately they're for the host
    // of llvm-config, not the target that we're attempting to link.
    let mut cmd = Command::new(&llvm_config);
    cmd.arg(llvm_link_arg).arg("--libs");

    if !is_crossed {
        cmd.arg("--system-libs");
    }
    cmd.args(&components);

    for lib in output(&mut cmd).split_whitespace() {
        let name = if lib.starts_with("-l") {
            &lib[2..]
        } else if lib.starts_with("-") {
            &lib[1..]
        } else if Path::new(lib).exists() {
            // On MSVC llvm-config will print the full name to libraries, but
            // we're only interested in the name part
            let name = Path::new(lib).file_name().unwrap().to_str().unwrap();
            name.trim_end_matches(".lib")
        } else if lib.ends_with(".lib") {
            // Some MSVC libraries just come up with `.lib` tacked on, so chop
            // that off
            lib.trim_end_matches(".lib")
        } else {
            continue;
        };

        // Don't need or want this library, but LLVM's CMake build system
        // doesn't provide a way to disable it, so filter it here even though we
        // may or may not have built it. We don't reference anything from this
        // library and it otherwise may just pull in extra dependencies on
        // libedit which we don't want
        if name == "LLVMLineEditor" {
            continue;
        }

        let kind = if name.starts_with("LLVM") {
            llvm_kind
        } else {
            "dylib"
        };
        println!("cargo:rustc-link-lib={}={}", kind, name);
    }

    // LLVM ldflags
    //
    // If we're a cross-compile of LLVM then unfortunately we can't trust these
    // ldflags (largely where all the LLVM libs are located). Currently just
    // hack around this by replacing the host triple with the target and pray
    // that those -L directories are the same!
    let mut cmd = Command::new(&llvm_config);
    cmd.arg(llvm_link_arg).arg("--ldflags");
    for lib in output(&mut cmd).split_whitespace() {
        if lib.starts_with("-LIBPATH:") {
            println!("cargo:rustc-link-search=native={}", &lib[9..]);
        } else if is_crossed {
            if lib.starts_with("-L") {
                println!("cargo:rustc-link-search=native={}",
                         lib[2..].replace(&host, &target));
            }
        } else if lib.starts_with("-l") {
            println!("cargo:rustc-link-lib={}", &lib[2..]);
        } else if lib.starts_with("-L") {
            println!("cargo:rustc-link-search=native={}", &lib[2..]);
        }
    }

    // Some LLVM linker flags (-L and -l) may be needed even when linking
    // librustc_llvm, for example when using static libc++, we may need to
    // manually specify the library search path and -ldl -lpthread as link
    // dependencies.
    let llvm_linker_flags = env::var_os("LLVM_LINKER_FLAGS");
    if let Some(s) = llvm_linker_flags {
        for lib in s.into_string().unwrap().split_whitespace() {
            if lib.starts_with("-l") {
                println!("cargo:rustc-link-lib={}", &lib[2..]);
            } else if lib.starts_with("-L") {
                println!("cargo:rustc-link-search=native={}", &lib[2..]);
            }
        }
    }

    let llvm_static_stdcpp = env::var_os("LLVM_STATIC_STDCPP");
    let llvm_use_libcxx = env::var_os("LLVM_USE_LIBCXX");

    let stdcppname = if target.contains("openbsd") {
        if target.contains("sparc64") {
            "estdc++"
        } else {
            "c++"
        }
    } else if target.contains("freebsd") {
        "c++"
    } else if target.contains("darwin") {
        "c++"
    } else if target.contains("netbsd") && llvm_static_stdcpp.is_some() {
        // NetBSD uses a separate library when relocation is required
        "stdc++_pic"
    } else if llvm_use_libcxx.is_some() {
        "c++"
    } else {
        "stdc++"
    };

    // C++ runtime library
    if !target.contains("msvc") {
        if let Some(s) = llvm_static_stdcpp {
            assert!(!cxxflags.contains("stdlib=libc++"));
            let path = PathBuf::from(s);
            println!("cargo:rustc-link-search=native={}",
                     path.parent().unwrap().display());
            if target.contains("windows") {
                println!("cargo:rustc-link-lib=static-nobundle={}", stdcppname);
            } else {
                println!("cargo:rustc-link-lib=static={}", stdcppname);
            }
        } else if cxxflags.contains("stdlib=libc++") {
            println!("cargo:rustc-link-lib=c++");
        } else {
            println!("cargo:rustc-link-lib={}", stdcppname);
        }
    }

    // LLVM requires symbols from this library, but apparently they're not printed
    // during llvm-config?
    if target.contains("windows-gnu") {
        println!("cargo:rustc-link-lib=static-nobundle=gcc_s");
        println!("cargo:rustc-link-lib=static-nobundle=pthread");
        println!("cargo:rustc-link-lib=dylib=uuid");
    }
}
