use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::core::compiler::unit_dependencies;
use crate::core::compiler::UnitInterner;
use crate::core::compiler::{
    BuildConfig, BuildContext, CompileKind, CompileMode, Context, ProfileKind,
};
use crate::core::profiles::UnitFor;
use crate::core::Workspace;
use crate::ops;
use crate::util::errors::{CargoResult, CargoResultExt};
use crate::util::paths;
use crate::util::Config;

pub struct CleanOptions<'a> {
    pub config: &'a Config,
    /// A list of packages to clean. If empty, everything is cleaned.
    pub spec: Vec<String>,
    /// The target arch triple to clean, or None for the host arch
    pub target: Option<String>,
    /// Whether to clean the release directory
    pub profile_specified: bool,
    /// Whether to clean the directory of a certain build profile
    pub profile_kind: ProfileKind,
    /// Whether to just clean the doc directory
    pub doc: bool,
}

/// Cleans the package's build artifacts.
pub fn clean(ws: &Workspace<'_>, opts: &CleanOptions<'_>) -> CargoResult<()> {
    let mut target_dir = ws.target_dir();
    let config = ws.config();

    // If the doc option is set, we just want to delete the doc directory.
    if opts.doc {
        target_dir = target_dir.join("doc");
        return rm_rf(&target_dir.into_path_unlocked(), config);
    }

    let profiles = ws.profiles();

    // Check for whether the profile is defined.
    let _ = profiles.base_profile(&opts.profile_kind)?;

    if opts.profile_specified {
        // After parsing profiles we know the dir-name of the profile, if a profile
        // was passed from the command line. If so, delete only the directory of
        // that profile.
        let dir_name = profiles.get_dir_name(&opts.profile_kind);
        target_dir = target_dir.join(dir_name);
    }

    // If we have a spec, then we need to delete some packages, otherwise, just
    // remove the whole target directory and be done with it!
    //
    // Note that we don't bother grabbing a lock here as we're just going to
    // blow it all away anyway.
    if opts.spec.is_empty() {
        return rm_rf(&target_dir.into_path_unlocked(), config);
    }
    let (packages, resolve) = ops::resolve_ws(ws)?;

    let interner = UnitInterner::new();
    let mut build_config = BuildConfig::new(config, Some(1), &opts.target, CompileMode::Build)?;
    let profile_kind = opts.profile_kind.clone();
    build_config.profile_kind = profile_kind.clone();
    let bcx = BuildContext::new(
        ws,
        &packages,
        opts.config,
        &build_config,
        profiles,
        &interner,
        HashMap::new(),
    )?;
    let mut units = Vec::new();

    for spec in opts.spec.iter() {
        // Translate the spec to a Package
        let pkgid = resolve.query(spec)?;
        let pkg = packages.get_one(pkgid)?;

        // Generate all relevant `Unit` targets for this package
        for target in pkg.targets() {
            for kind in [CompileKind::Host, build_config.requested_kind].iter() {
                for mode in CompileMode::all_modes() {
                    for unit_for in UnitFor::all_values() {
                        let profile = if mode.is_run_custom_build() {
                            profiles.get_profile_run_custom_build(&profiles.get_profile(
                                pkg.package_id(),
                                ws.is_member(pkg),
                                *unit_for,
                                CompileMode::Build,
                                profile_kind.clone(),
                            ))
                        } else {
                            profiles.get_profile(
                                pkg.package_id(),
                                ws.is_member(pkg),
                                *unit_for,
                                *mode,
                                profile_kind.clone(),
                            )
                        };
                        let features = resolve.features_sorted(pkg.package_id());
                        units.push(bcx.units.intern(
                            pkg, target, profile, *kind, *mode, features, /*is_std*/ false,
                        ));
                    }
                }
            }
        }
    }

    let unit_dependencies =
        unit_dependencies::build_unit_dependencies(&bcx, &resolve, None, &units, &[])?;
    let mut cx = Context::new(config, &bcx, unit_dependencies, build_config.requested_kind)?;
    cx.prepare_units(None, &units)?;

    for unit in units.iter() {
        if unit.mode.is_doc() || unit.mode.is_doc_test() {
            // Cleaning individual rustdoc crates is currently not supported.
            // For example, the search index would need to be rebuilt to fully
            // remove it (otherwise you're left with lots of broken links).
            // Doc tests produce no output.
            continue;
        }
        rm_rf(&cx.files().fingerprint_dir(unit), config)?;
        if unit.target.is_custom_build() {
            if unit.mode.is_run_custom_build() {
                rm_rf(&cx.files().build_script_out_dir(unit), config)?;
            } else {
                rm_rf(&cx.files().build_script_dir(unit), config)?;
            }
            continue;
        }

        for output in cx.outputs(unit)?.iter() {
            rm_rf(&output.path, config)?;
            if let Some(ref dst) = output.hardlink {
                rm_rf(dst, config)?;
            }
        }
    }

    Ok(())
}

fn rm_rf(path: &Path, config: &Config) -> CargoResult<()> {
    let m = fs::metadata(path);
    if m.as_ref().map(|s| s.is_dir()).unwrap_or(false) {
        config
            .shell()
            .verbose(|shell| shell.status("Removing", path.display()))?;
        paths::remove_dir_all(path)
            .chain_err(|| failure::format_err!("could not remove build directory"))?;
    } else if m.is_ok() {
        config
            .shell()
            .verbose(|shell| shell.status("Removing", path.display()))?;
        paths::remove_file(path)
            .chain_err(|| failure::format_err!("failed to remove build artifact"))?;
    }
    Ok(())
}
