#![feature(rustc_private)]

extern crate rustc;
extern crate rustc_interface;
extern crate rustc_driver;
extern crate syntax;

use rustc::session::DiagnosticOutput;
use rustc::session::config::{Input, Options,
                             OutputType, OutputTypes};
use rustc_interface::interface;
use syntax::source_map::FileName;

use std::path::PathBuf;

fn main() {
    let src = r#"
    fn main() {}
    "#;

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 4 {
        panic!("expected rustc path");
    }

    let tmpdir = PathBuf::from(&args[1]);

    let mut sysroot = PathBuf::from(&args[3]);
    sysroot.pop();
    sysroot.pop();

    compile(src.to_string(), tmpdir.join("out"), sysroot.clone());

    compile(src.to_string(), tmpdir.join("out"), sysroot.clone());
}

fn compile(code: String, output: PathBuf, sysroot: PathBuf) {
    let mut opts = Options::default();
    opts.output_types = OutputTypes::new(&[(OutputType::Exe, None)]);
    opts.maybe_sysroot = Some(sysroot);

    if let Ok(linker) = std::env::var("RUSTC_LINKER") {
        opts.cg.linker = Some(linker.into());
    }

    let name = FileName::anon_source_code(&code);
    let input = Input::Str { name, input: code };

    let config = interface::Config {
        opts,
        crate_cfg: Default::default(),
        input,
        input_path: None,
        output_file: Some(output),
        output_dir: None,
        file_loader: None,
        diagnostic_output: DiagnosticOutput::Default,
        stderr: None,
        crate_name: None,
        lint_caps: Default::default(),
        register_lints: None,
        override_queries: None,
        registry: rustc_driver::diagnostics_registry(),
    };

    interface::run_compiler(config, |compiler| {
        // This runs all the passes prior to linking, too.
        compiler.link().ok();
    });
}
