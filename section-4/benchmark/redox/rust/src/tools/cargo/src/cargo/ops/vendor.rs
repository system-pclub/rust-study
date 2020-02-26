use crate::core::shell::Verbosity;
use crate::core::{GitReference, Workspace};
use crate::ops;
use crate::sources::path::PathSource;
use crate::util::Sha256;
use crate::util::{paths, CargoResult, CargoResultExt, Config};
use failure::bail;
use serde::Serialize;
use std::collections::HashSet;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct VendorOptions<'a> {
    pub no_delete: bool,
    pub destination: &'a Path,
    pub extra: Vec<PathBuf>,
}

pub fn vendor(ws: &Workspace<'_>, opts: &VendorOptions<'_>) -> CargoResult<()> {
    let mut extra_workspaces = Vec::new();
    for extra in opts.extra.iter() {
        let extra = ws.config().cwd().join(extra);
        let ws = Workspace::new(&extra, ws.config())?;
        extra_workspaces.push(ws);
    }
    let workspaces = extra_workspaces.iter().chain(Some(ws)).collect::<Vec<_>>();
    let vendor_config =
        sync(ws.config(), &workspaces, opts).chain_err(|| "failed to sync".to_string())?;

    let shell = ws.config().shell();
    if shell.verbosity() != Verbosity::Quiet {
        eprint!("To use vendored sources, add this to your .cargo/config for this project:\n\n");
        print!("{}", &toml::to_string(&vendor_config).unwrap());
    }

    Ok(())
}

#[derive(Serialize)]
struct VendorConfig {
    source: BTreeMap<String, VendorSource>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase", untagged)]
enum VendorSource {
    Directory {
        directory: PathBuf,
    },
    Registry {
        registry: Option<String>,
        #[serde(rename = "replace-with")]
        replace_with: String,
    },
    Git {
        git: String,
        branch: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        #[serde(rename = "replace-with")]
        replace_with: String,
    },
}

fn sync(
    config: &Config,
    workspaces: &[&Workspace<'_>],
    opts: &VendorOptions<'_>,
) -> CargoResult<VendorConfig> {
    let canonical_destination = opts.destination.canonicalize();
    let canonical_destination = canonical_destination
        .as_ref()
        .map(|p| &**p)
        .unwrap_or(opts.destination);

    paths::create_dir_all(&canonical_destination)?;
    let mut to_remove = HashSet::new();
    if !opts.no_delete {
        for entry in canonical_destination.read_dir()? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_str()
                .map_or(false, |s| s.starts_with('.'))
            {
                to_remove.insert(entry.path());
            }
        }
    }

    // First up attempt to work around rust-lang/cargo#5956. Apparently build
    // artifacts sprout up in Cargo's global cache for whatever reason, although
    // it's unsure what tool is causing these issues at this time. For now we
    // apply a heavy-hammer approach which is to delete Cargo's unpacked version
    // of each crate to start off with. After we do this we'll re-resolve and
    // redownload again, which should trigger Cargo to re-extract all the
    // crates.
    //
    // Note that errors are largely ignored here as this is a best-effort
    // attempt. If anything fails here we basically just move on to the next
    // crate to work with.
    for ws in workspaces {
        let (packages, resolve) =
            ops::resolve_ws(ws).chain_err(|| "failed to load pkg lockfile")?;

        packages
            .get_many(resolve.iter())
            .chain_err(|| "failed to download packages")?;

        for pkg in resolve.iter() {
            // Don't delete actual source code!
            if pkg.source_id().is_path() {
                if let Ok(path) = pkg.source_id().url().to_file_path() {
                    if let Ok(path) = path.canonicalize() {
                        to_remove.remove(&path);
                    }
                }
                continue;
            }
            if pkg.source_id().is_git() {
                continue;
            }
            if let Ok(pkg) = packages.get_one(pkg) {
                drop(fs::remove_dir_all(pkg.manifest_path().parent().unwrap()));
            }
        }
    }

    let mut checksums = HashMap::new();
    let mut ids = BTreeMap::new();

    // Next up let's actually download all crates and start storing internal
    // tables about them.
    for ws in workspaces {
        let (packages, resolve) =
            ops::resolve_ws(ws).chain_err(|| "failed to load pkg lockfile")?;

        packages
            .get_many(resolve.iter())
            .chain_err(|| "failed to download packages")?;

        for pkg in resolve.iter() {
            // No need to vendor path crates since they're already in the
            // repository
            if pkg.source_id().is_path() {
                continue;
            }
            ids.insert(
                pkg,
                packages
                    .get_one(pkg)
                    .chain_err(|| "failed to fetch package")?
                    .clone(),
            );

            checksums.insert(pkg, resolve.checksums().get(&pkg).cloned());
        }
    }

    let mut versions = HashMap::new();
    for id in ids.keys() {
        let map = versions.entry(id.name()).or_insert_with(BTreeMap::default);
        if let Some(prev) = map.get(&id.version()) {
            bail!(
                "found duplicate version of package `{} v{}` \
                 vendored from two sources:\n\
                 \n\
                 \tsource 1: {}\n\
                 \tsource 2: {}",
                id.name(),
                id.version(),
                prev,
                id.source_id()
            );
        }
        map.insert(id.version(), id.source_id());
    }

    let mut sources = BTreeSet::new();
    for (id, pkg) in ids.iter() {
        // Next up, copy it to the vendor directory
        let src = pkg
            .manifest_path()
            .parent()
            .expect("manifest_path should point to a file");
        let max_version = *versions[&id.name()].iter().rev().next().unwrap().0;
        let dir_has_version_suffix = id.version() != max_version;
        let dst_name = if dir_has_version_suffix {
            // Eg vendor/futures-0.1.13
            format!("{}-{}", id.name(), id.version())
        } else {
            // Eg vendor/futures
            id.name().to_string()
        };

        sources.insert(id.source_id());
        let dst = canonical_destination.join(&dst_name);
        to_remove.remove(&dst);
        let cksum = dst.join(".cargo-checksum.json");
        if dir_has_version_suffix && cksum.exists() {
            // Always re-copy directory without version suffix in case the version changed
            continue;
        }

        config.shell().status(
            "Vendoring",
            &format!("{} ({}) to {}", id, src.to_string_lossy(), dst.display()),
        )?;

        let _ = fs::remove_dir_all(&dst);
        let pathsource = PathSource::new(src, id.source_id(), config);
        let paths = pathsource.list_files(pkg)?;
        let mut map = BTreeMap::new();
        cp_sources(src, &paths, &dst, &mut map)
            .chain_err(|| format!("failed to copy over vendored sources for: {}", id))?;

        // Finally, emit the metadata about this package
        let json = serde_json::json!({
            "package": checksums.get(id),
            "files": map,
        });

        File::create(&cksum)?.write_all(json.to_string().as_bytes())?;
    }

    for path in to_remove {
        if path.is_dir() {
            paths::remove_dir_all(&path)?;
        } else {
            paths::remove_file(&path)?;
        }
    }

    // add our vendored source
    let mut config = BTreeMap::new();

    let merged_source_name = "vendored-sources";
    config.insert(
        merged_source_name.to_string(),
        VendorSource::Directory {
            directory: canonical_destination.to_path_buf(),
        },
    );

    // replace original sources with vendor
    for source_id in sources {
        let name = if source_id.is_default_registry() {
            "crates-io".to_string()
        } else {
            source_id.url().to_string()
        };

        let source = if source_id.is_default_registry() {
            VendorSource::Registry {
                registry: None,
                replace_with: merged_source_name.to_string(),
            }
        } else if source_id.is_git() {
            let mut branch = None;
            let mut tag = None;
            let mut rev = None;
            if let Some(reference) = source_id.git_reference() {
                match *reference {
                    GitReference::Branch(ref b) => branch = Some(b.clone()),
                    GitReference::Tag(ref t) => tag = Some(t.clone()),
                    GitReference::Rev(ref r) => rev = Some(r.clone()),
                }
            }
            VendorSource::Git {
                git: source_id.url().to_string(),
                branch,
                tag,
                rev,
                replace_with: merged_source_name.to_string(),
            }
        } else {
            panic!("Invalid source ID: {}", source_id)
        };
        config.insert(name, source);
    }

    Ok(VendorConfig { source: config })
}

fn cp_sources(
    src: &Path,
    paths: &[PathBuf],
    dst: &Path,
    cksums: &mut BTreeMap<String, String>,
) -> CargoResult<()> {
    for p in paths {
        let relative = p.strip_prefix(&src).unwrap();

        match relative.to_str() {
            // Skip git config files as they're not relevant to builds most of
            // the time and if we respect them (e.g.  in git) then it'll
            // probably mess with the checksums when a vendor dir is checked
            // into someone else's source control
            Some(".gitattributes") | Some(".gitignore") | Some(".git") => continue,

            // Temporary Cargo files
            Some(".cargo-ok") => continue,

            // Skip patch-style orig/rej files. Published crates on crates.io
            // have `Cargo.toml.orig` which we don't want to use here and
            // otherwise these are rarely used as part of the build process.
            Some(filename) => {
                if filename.ends_with(".orig") || filename.ends_with(".rej") {
                    continue;
                }
            }
            _ => {}
        };

        // Join pathname components individually to make sure that the joined
        // path uses the correct directory separators everywhere, since
        // `relative` may use Unix-style and `dst` may require Windows-style
        // backslashes.
        let dst = relative
            .iter()
            .fold(dst.to_owned(), |acc, component| acc.join(&component));

        paths::create_dir_all(dst.parent().unwrap())?;

        fs::copy(&p, &dst)
            .chain_err(|| format!("failed to copy `{}` to `{}`", p.display(), dst.display()))?;
        let cksum = Sha256::new().update_path(dst)?.finish_hex();
        cksums.insert(relative.to_str().unwrap().replace("\\", "/"), cksum);
    }
    Ok(())
}
