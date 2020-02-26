//! This module implements Cargo conventions for directory layout:
//!
//!  * `src/lib.rs` is a library
//!  * `src/main.rs` is a binary
//!  * `src/bin/*.rs` are binaries
//!  * `examples/*.rs` are examples
//!  * `tests/*.rs` are integration tests
//!  * `benches/*.rs` are benchmarks
//!
//! It is a bit tricky because we need match explicit information from `Cargo.toml`
//! with implicit info in directory layout.

use std::collections::HashSet;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use super::{
    LibKind, PathValue, StringOrBool, StringOrVec, TomlBenchTarget, TomlBinTarget,
    TomlExampleTarget, TomlLibTarget, TomlManifest, TomlTarget, TomlTestTarget,
};
use crate::core::{compiler, Edition, Feature, Features, Target};
use crate::util::errors::{CargoResult, CargoResultExt};

pub fn targets(
    features: &Features,
    manifest: &TomlManifest,
    package_name: &str,
    package_root: &Path,
    edition: Edition,
    custom_build: &Option<StringOrBool>,
    metabuild: &Option<StringOrVec>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> CargoResult<Vec<Target>> {
    let mut targets = Vec::new();

    let has_lib;

    if let Some(target) = clean_lib(
        features,
        manifest.lib.as_ref(),
        package_root,
        package_name,
        edition,
        warnings,
    )? {
        targets.push(target);
        has_lib = true;
    } else {
        has_lib = false;
    }

    let package = manifest
        .package
        .as_ref()
        .or_else(|| manifest.project.as_ref())
        .ok_or_else(|| failure::format_err!("manifest has no `package` (or `project`)"))?;

    targets.extend(clean_bins(
        features,
        manifest.bin.as_ref(),
        package_root,
        package_name,
        edition,
        package.autobins,
        warnings,
        errors,
        has_lib,
    )?);

    targets.extend(clean_examples(
        features,
        manifest.example.as_ref(),
        package_root,
        edition,
        package.autoexamples,
        warnings,
        errors,
    )?);

    targets.extend(clean_tests(
        features,
        manifest.test.as_ref(),
        package_root,
        edition,
        package.autotests,
        warnings,
        errors,
    )?);

    targets.extend(clean_benches(
        features,
        manifest.bench.as_ref(),
        package_root,
        edition,
        package.autobenches,
        warnings,
        errors,
    )?);

    // processing the custom build script
    if let Some(custom_build) = manifest.maybe_custom_build(custom_build, package_root) {
        if metabuild.is_some() {
            failure::bail!("cannot specify both `metabuild` and `build`");
        }
        let name = format!(
            "build-script-{}",
            custom_build
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        );
        targets.push(Target::custom_build_target(
            &name,
            package_root.join(custom_build),
            edition,
        ));
    }
    if let Some(metabuild) = metabuild {
        // Verify names match available build deps.
        let bdeps = manifest.build_dependencies.as_ref();
        for name in &metabuild.0 {
            if !bdeps.map_or(false, |bd| bd.contains_key(name)) {
                failure::bail!(
                    "metabuild package `{}` must be specified in `build-dependencies`",
                    name
                );
            }
        }

        targets.push(Target::metabuild_target(&format!(
            "metabuild-{}",
            package.name
        )));
    }

    Ok(targets)
}

fn clean_lib(
    features: &Features,
    toml_lib: Option<&TomlLibTarget>,
    package_root: &Path,
    package_name: &str,
    edition: Edition,
    warnings: &mut Vec<String>,
) -> CargoResult<Option<Target>> {
    let inferred = inferred_lib(package_root);
    let lib = match toml_lib {
        Some(lib) => {
            if let Some(ref name) = lib.name {
                // XXX: other code paths dodge this validation
                if name.contains('-') {
                    failure::bail!("library target names cannot contain hyphens: {}", name)
                }
            }
            Some(TomlTarget {
                name: lib.name.clone().or_else(|| Some(package_name.to_owned())),
                ..lib.clone()
            })
        }
        None => inferred.as_ref().map(|lib| TomlTarget {
            name: Some(package_name.to_string()),
            path: Some(PathValue(lib.clone())),
            ..TomlTarget::new()
        }),
    };

    let lib = match lib {
        Some(ref lib) => lib,
        None => return Ok(None),
    };

    validate_has_name(lib, "library", "lib")?;

    let path = match (lib.path.as_ref(), inferred) {
        (Some(path), _) => package_root.join(&path.0),
        (None, Some(path)) => path,
        (None, None) => {
            let legacy_path = package_root.join("src").join(format!("{}.rs", lib.name()));
            if edition == Edition::Edition2015 && legacy_path.exists() {
                warnings.push(format!(
                    "path `{}` was erroneously implicitly accepted for library `{}`,\n\
                     please rename the file to `src/lib.rs` or set lib.path in Cargo.toml",
                    legacy_path.display(),
                    lib.name()
                ));
                legacy_path
            } else {
                failure::bail!(
                    "can't find library `{}`, \
                     rename file to `src/lib.rs` or specify lib.path",
                    lib.name()
                )
            }
        }
    };

    // Per the Macros 1.1 RFC:
    //
    // > Initially if a crate is compiled with the `proc-macro` crate type
    // > (and possibly others) it will forbid exporting any items in the
    // > crate other than those functions tagged #[proc_macro_derive] and
    // > those functions must also be placed at the crate root.
    //
    // A plugin requires exporting plugin_registrar so a crate cannot be
    // both at once.
    let crate_types = match (lib.crate_types(), lib.plugin, lib.proc_macro()) {
        (Some(kinds), _, _) if kinds.contains(&"proc-macro".to_string()) => {
            if let Some(true) = lib.plugin {
                // This is a warning to retain backwards compatibility.
                warnings.push(format!(
                    "proc-macro library `{}` should not specify `plugin = true`",
                    lib.name()
                ));
            }
            warnings.push(format!(
                "library `{}` should only specify `proc-macro = true` instead of setting `crate-type`",
                lib.name()
            ));
            if kinds.len() > 1 {
                failure::bail!("cannot mix `proc-macro` crate type with others");
            }
            vec![LibKind::ProcMacro]
        }
        (_, Some(true), Some(true)) => {
            failure::bail!("`lib.plugin` and `lib.proc-macro` cannot both be `true`")
        }
        (Some(kinds), _, _) => kinds.iter().map(|s| s.into()).collect(),
        (None, Some(true), _) => vec![LibKind::Dylib],
        (None, _, Some(true)) => vec![LibKind::ProcMacro],
        (None, _, _) => vec![LibKind::Lib],
    };

    let mut target = Target::lib_target(&lib.name(), crate_types, path, edition);
    configure(features, lib, &mut target)?;
    Ok(Some(target))
}

fn clean_bins(
    features: &Features,
    toml_bins: Option<&Vec<TomlBinTarget>>,
    package_root: &Path,
    package_name: &str,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
    has_lib: bool,
) -> CargoResult<Vec<Target>> {
    let inferred = inferred_bins(package_root, package_name);

    let bins = toml_targets_and_inferred(
        toml_bins,
        &inferred,
        package_root,
        autodiscover,
        edition,
        warnings,
        "binary",
        "bin",
        "autobins",
    );

    for bin in &bins {
        validate_has_name(bin, "binary", "bin")?;

        let name = bin.name();

        if let Some(crate_types) = bin.crate_types() {
            if !crate_types.is_empty() {
                errors.push(format!(
                    "the target `{}` is a binary and can't have any \
                     crate-types set (currently \"{}\")",
                    name,
                    crate_types.join(", ")
                ));
            }
        }

        if bin.proc_macro() == Some(true) {
            errors.push(format!(
                "the target `{}` is a binary and can't have `proc-macro` \
                 set `true`",
                name
            ));
        }

        if compiler::is_bad_artifact_name(&name) {
            failure::bail!("the binary target name `{}` is forbidden", name)
        }
    }

    validate_unique_names(&bins, "binary")?;

    let mut result = Vec::new();
    for bin in &bins {
        let path = target_path(bin, &inferred, "bin", package_root, edition, &mut |_| {
            if let Some(legacy_path) = legacy_bin_path(package_root, &bin.name(), has_lib) {
                warnings.push(format!(
                    "path `{}` was erroneously implicitly accepted for binary `{}`,\n\
                     please set bin.path in Cargo.toml",
                    legacy_path.display(),
                    bin.name()
                ));
                Some(legacy_path)
            } else {
                None
            }
        });
        let path = match path {
            Ok(path) => path,
            Err(e) => failure::bail!("{}", e),
        };

        let mut target =
            Target::bin_target(&bin.name(), path, bin.required_features.clone(), edition);
        configure(features, bin, &mut target)?;
        result.push(target);
    }
    return Ok(result);

    fn legacy_bin_path(package_root: &Path, name: &str, has_lib: bool) -> Option<PathBuf> {
        if !has_lib {
            let path = package_root.join("src").join(format!("{}.rs", name));
            if path.exists() {
                return Some(path);
            }
        }
        let path = package_root.join("src").join("main.rs");
        if path.exists() {
            return Some(path);
        }

        let path = package_root.join("src").join("bin").join("main.rs");
        if path.exists() {
            return Some(path);
        }
        None
    }
}

fn clean_examples(
    features: &Features,
    toml_examples: Option<&Vec<TomlExampleTarget>>,
    package_root: &Path,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> CargoResult<Vec<Target>> {
    let inferred = infer_from_directory(&package_root.join("examples"));

    let targets = clean_targets(
        "example",
        "example",
        toml_examples,
        &inferred,
        package_root,
        edition,
        autodiscover,
        warnings,
        errors,
        "autoexamples",
    )?;

    let mut result = Vec::new();
    for (path, toml) in targets {
        let crate_types = match toml.crate_types() {
            Some(kinds) => kinds.iter().map(|s| s.into()).collect(),
            None => Vec::new(),
        };

        let mut target = Target::example_target(
            &toml.name(),
            crate_types,
            path,
            toml.required_features.clone(),
            edition,
        );
        configure(features, &toml, &mut target)?;
        result.push(target);
    }

    Ok(result)
}

fn clean_tests(
    features: &Features,
    toml_tests: Option<&Vec<TomlTestTarget>>,
    package_root: &Path,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> CargoResult<Vec<Target>> {
    let inferred = infer_from_directory(&package_root.join("tests"));

    let targets = clean_targets(
        "test",
        "test",
        toml_tests,
        &inferred,
        package_root,
        edition,
        autodiscover,
        warnings,
        errors,
        "autotests",
    )?;

    let mut result = Vec::new();
    for (path, toml) in targets {
        let mut target =
            Target::test_target(&toml.name(), path, toml.required_features.clone(), edition);
        configure(features, &toml, &mut target)?;
        result.push(target);
    }
    Ok(result)
}

fn clean_benches(
    features: &Features,
    toml_benches: Option<&Vec<TomlBenchTarget>>,
    package_root: &Path,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> CargoResult<Vec<Target>> {
    let mut legacy_warnings = vec![];

    let targets = {
        let mut legacy_bench_path = |bench: &TomlTarget| {
            let legacy_path = package_root.join("src").join("bench.rs");
            if !(bench.name() == "bench" && legacy_path.exists()) {
                return None;
            }
            legacy_warnings.push(format!(
                "path `{}` was erroneously implicitly accepted for benchmark `{}`,\n\
                 please set bench.path in Cargo.toml",
                legacy_path.display(),
                bench.name()
            ));
            Some(legacy_path)
        };

        let inferred = infer_from_directory(&package_root.join("benches"));

        clean_targets_with_legacy_path(
            "benchmark",
            "bench",
            toml_benches,
            &inferred,
            package_root,
            edition,
            autodiscover,
            warnings,
            errors,
            &mut legacy_bench_path,
            "autobenches",
        )?
    };

    warnings.append(&mut legacy_warnings);

    let mut result = Vec::new();
    for (path, toml) in targets {
        let mut target =
            Target::bench_target(&toml.name(), path, toml.required_features.clone(), edition);
        configure(features, &toml, &mut target)?;
        result.push(target);
    }

    Ok(result)
}

fn clean_targets(
    target_kind_human: &str,
    target_kind: &str,
    toml_targets: Option<&Vec<TomlTarget>>,
    inferred: &[(String, PathBuf)],
    package_root: &Path,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
    autodiscover_flag_name: &str,
) -> CargoResult<Vec<(PathBuf, TomlTarget)>> {
    clean_targets_with_legacy_path(
        target_kind_human,
        target_kind,
        toml_targets,
        inferred,
        package_root,
        edition,
        autodiscover,
        warnings,
        errors,
        &mut |_| None,
        autodiscover_flag_name,
    )
}

fn clean_targets_with_legacy_path(
    target_kind_human: &str,
    target_kind: &str,
    toml_targets: Option<&Vec<TomlTarget>>,
    inferred: &[(String, PathBuf)],
    package_root: &Path,
    edition: Edition,
    autodiscover: Option<bool>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<String>,
    legacy_path: &mut dyn FnMut(&TomlTarget) -> Option<PathBuf>,
    autodiscover_flag_name: &str,
) -> CargoResult<Vec<(PathBuf, TomlTarget)>> {
    let toml_targets = toml_targets_and_inferred(
        toml_targets,
        inferred,
        package_root,
        autodiscover,
        edition,
        warnings,
        target_kind_human,
        target_kind,
        autodiscover_flag_name,
    );

    for target in &toml_targets {
        validate_has_name(target, target_kind_human, target_kind)?;
    }

    validate_unique_names(&toml_targets, target_kind)?;
    let mut result = Vec::new();
    for target in toml_targets {
        let path = target_path(
            &target,
            inferred,
            target_kind,
            package_root,
            edition,
            legacy_path,
        );
        let path = match path {
            Ok(path) => path,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        result.push((path, target));
    }
    Ok(result)
}

fn inferred_lib(package_root: &Path) -> Option<PathBuf> {
    let lib = package_root.join("src").join("lib.rs");
    if fs::metadata(&lib).is_ok() {
        Some(lib)
    } else {
        None
    }
}

fn inferred_bins(package_root: &Path, package_name: &str) -> Vec<(String, PathBuf)> {
    let main = package_root.join("src").join("main.rs");
    let mut result = Vec::new();
    if main.exists() {
        result.push((package_name.to_string(), main));
    }
    result.extend(infer_from_directory(&package_root.join("src").join("bin")));

    result
}

fn infer_from_directory(directory: &Path) -> Vec<(String, PathBuf)> {
    let entries = match fs::read_dir(directory) {
        Err(_) => return Vec::new(),
        Ok(dir) => dir,
    };

    entries
        .filter_map(|e| e.ok())
        .filter(is_not_dotfile)
        .filter_map(|d| infer_any(&d))
        .collect()
}

fn infer_any(entry: &DirEntry) -> Option<(String, PathBuf)> {
    if entry.path().extension().and_then(|p| p.to_str()) == Some("rs") {
        infer_file(entry)
    } else if entry.file_type().map(|t| t.is_dir()).ok() == Some(true) {
        infer_subdirectory(entry)
    } else {
        None
    }
}

fn infer_file(entry: &DirEntry) -> Option<(String, PathBuf)> {
    let path = entry.path();
    path.file_stem()
        .and_then(|p| p.to_str())
        .map(|p| (p.to_owned(), path.clone()))
}

fn infer_subdirectory(entry: &DirEntry) -> Option<(String, PathBuf)> {
    let path = entry.path();
    let main = path.join("main.rs");
    let name = path.file_name().and_then(|n| n.to_str());
    match (name, main.exists()) {
        (Some(name), true) => Some((name.to_owned(), main)),
        _ => None,
    }
}

fn is_not_dotfile(entry: &DirEntry) -> bool {
    entry.file_name().to_str().map(|s| s.starts_with('.')) == Some(false)
}

fn toml_targets_and_inferred(
    toml_targets: Option<&Vec<TomlTarget>>,
    inferred: &[(String, PathBuf)],
    package_root: &Path,
    autodiscover: Option<bool>,
    edition: Edition,
    warnings: &mut Vec<String>,
    target_kind_human: &str,
    target_kind: &str,
    autodiscover_flag_name: &str,
) -> Vec<TomlTarget> {
    let inferred_targets = inferred_to_toml_targets(inferred);
    match toml_targets {
        None => {
            if let Some(false) = autodiscover {
                vec![]
            } else {
                inferred_targets
            }
        }
        Some(targets) => {
            let mut targets = targets.clone();

            let target_path =
                |target: &TomlTarget| target.path.clone().map(|p| package_root.join(p.0));

            let mut seen_names = HashSet::new();
            let mut seen_paths = HashSet::new();
            for target in targets.iter() {
                seen_names.insert(target.name.clone());
                seen_paths.insert(target_path(target));
            }

            let mut rem_targets = vec![];
            for target in inferred_targets {
                if !seen_names.contains(&target.name) && !seen_paths.contains(&target_path(&target))
                {
                    rem_targets.push(target);
                }
            }

            let autodiscover = match autodiscover {
                Some(autodiscover) => autodiscover,
                None => {
                    if edition == Edition::Edition2015 {
                        if !rem_targets.is_empty() {
                            let mut rem_targets_str = String::new();
                            for t in rem_targets.iter() {
                                if let Some(p) = t.path.clone() {
                                    rem_targets_str.push_str(&format!("* {}\n", p.0.display()))
                                }
                            }
                            warnings.push(format!(
                                "\
An explicit [[{section}]] section is specified in Cargo.toml which currently
disables Cargo from automatically inferring other {target_kind_human} targets.
This inference behavior will change in the Rust 2018 edition and the following
files will be included as a {target_kind_human} target:

{rem_targets_str}
This is likely to break cargo build or cargo test as these files may not be
ready to be compiled as a {target_kind_human} target today. You can future-proof yourself
and disable this warning by adding `{autodiscover_flag_name} = false` to your [package]
section. You may also move the files to a location where Cargo would not
automatically infer them to be a target, such as in subfolders.

For more information on this warning you can consult
https://github.com/rust-lang/cargo/issues/5330",
                                section = target_kind,
                                target_kind_human = target_kind_human,
                                rem_targets_str = rem_targets_str,
                                autodiscover_flag_name = autodiscover_flag_name,
                            ));
                        };
                        false
                    } else {
                        true
                    }
                }
            };

            if autodiscover {
                targets.append(&mut rem_targets);
            }

            targets
        }
    }
}

fn inferred_to_toml_targets(inferred: &[(String, PathBuf)]) -> Vec<TomlTarget> {
    inferred
        .iter()
        .map(|&(ref name, ref path)| TomlTarget {
            name: Some(name.clone()),
            path: Some(PathValue(path.clone())),
            ..TomlTarget::new()
        })
        .collect()
}

fn validate_has_name(
    target: &TomlTarget,
    target_kind_human: &str,
    target_kind: &str,
) -> CargoResult<()> {
    match target.name {
        Some(ref name) => {
            if name.trim().is_empty() {
                failure::bail!("{} target names cannot be empty", target_kind_human)
            }
        }
        None => failure::bail!(
            "{} target {}.name is required",
            target_kind_human,
            target_kind
        ),
    }

    Ok(())
}

/// Will check a list of toml targets, and make sure the target names are unique within a vector.
fn validate_unique_names(targets: &[TomlTarget], target_kind: &str) -> CargoResult<()> {
    let mut seen = HashSet::new();
    for name in targets.iter().map(|e| e.name()) {
        if !seen.insert(name.clone()) {
            failure::bail!(
                "found duplicate {target_kind} name {name}, \
                 but all {target_kind} targets must have a unique name",
                target_kind = target_kind,
                name = name
            );
        }
    }
    Ok(())
}

fn configure(features: &Features, toml: &TomlTarget, target: &mut Target) -> CargoResult<()> {
    let t2 = target.clone();
    target
        .set_tested(toml.test.unwrap_or_else(|| t2.tested()))
        .set_doc(toml.doc.unwrap_or_else(|| t2.documented()))
        .set_doctest(toml.doctest.unwrap_or_else(|| t2.doctested()))
        .set_benched(toml.bench.unwrap_or_else(|| t2.benched()))
        .set_harness(toml.harness.unwrap_or_else(|| t2.harness()))
        .set_proc_macro(toml.proc_macro.unwrap_or_else(|| t2.proc_macro()))
        .set_for_host(match (toml.plugin, toml.proc_macro()) {
            (None, None) => t2.for_host(),
            (Some(true), _) | (_, Some(true)) => true,
            (Some(false), _) | (_, Some(false)) => false,
        });
    if let Some(edition) = toml.edition.clone() {
        features
            .require(Feature::edition())
            .chain_err(|| "editions are unstable")?;
        target.set_edition(
            edition
                .parse()
                .chain_err(|| "failed to parse the `edition` key")?,
        );
    }
    Ok(())
}

fn target_path(
    target: &TomlTarget,
    inferred: &[(String, PathBuf)],
    target_kind: &str,
    package_root: &Path,
    edition: Edition,
    legacy_path: &mut dyn FnMut(&TomlTarget) -> Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(ref path) = target.path {
        // Should we verify that this path exists here?
        return Ok(package_root.join(&path.0));
    }
    let name = target.name();

    let mut matching = inferred
        .iter()
        .filter(|&&(ref n, _)| n == &name)
        .map(|&(_, ref p)| p.clone());

    let first = matching.next();
    let second = matching.next();
    match (first, second) {
        (Some(path), None) => Ok(path),
        (None, None) | (Some(_), Some(_)) => {
            if edition == Edition::Edition2015 {
                if let Some(path) = legacy_path(target) {
                    return Ok(path);
                }
            }
            Err(format!(
                "can't find `{name}` {target_kind}, specify {target_kind}.path",
                name = name,
                target_kind = target_kind
            ))
        }
        (None, Some(_)) => unreachable!(),
    }
}
