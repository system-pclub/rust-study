use std::cell::RefCell;
use std::collections::hash_map::{Entry, HashMap};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::slice;

use glob::glob;
use log::debug;
use url::Url;

use crate::core::features::Features;
use crate::core::profiles::Profiles;
use crate::core::registry::PackageRegistry;
use crate::core::{Dependency, PackageId, PackageIdSpec};
use crate::core::{EitherManifest, Package, SourceId, VirtualManifest};
use crate::ops;
use crate::sources::PathSource;
use crate::util::errors::{CargoResult, CargoResultExt, ManifestError};
use crate::util::paths;
use crate::util::toml::read_manifest;
use crate::util::{Config, Filesystem};

/// The core abstraction in Cargo for working with a workspace of crates.
///
/// A workspace is often created very early on and then threaded through all
/// other functions. It's typically through this object that the current
/// package is loaded and/or learned about.
#[derive(Debug)]
pub struct Workspace<'cfg> {
    config: &'cfg Config,

    // This path is a path to where the current cargo subcommand was invoked
    // from. That is the `--manifest-path` argument to Cargo, and
    // points to the "main crate" that we're going to worry about.
    current_manifest: PathBuf,

    // A list of packages found in this workspace. Always includes at least the
    // package mentioned by `current_manifest`.
    packages: Packages<'cfg>,

    // If this workspace includes more than one crate, this points to the root
    // of the workspace. This is `None` in the case that `[workspace]` is
    // missing, `package.workspace` is missing, and no `Cargo.toml` above
    // `current_manifest` was found on the filesystem with `[workspace]`.
    root_manifest: Option<PathBuf>,

    // Shared target directory for all the packages of this workspace.
    // `None` if the default path of `root/target` should be used.
    target_dir: Option<Filesystem>,

    // List of members in this workspace with a listing of all their manifest
    // paths. The packages themselves can be looked up through the `packages`
    // set above.
    members: Vec<PathBuf>,
    member_ids: HashSet<PackageId>,

    // The subset of `members` that are used by the
    // `build`, `check`, `test`, and `bench` subcommands
    // when no package is selected with `--package` / `-p` and `--workspace`
    // is not used.
    //
    // This is set by the `default-members` config
    // in the `[workspace]` section.
    // When unset, this is the same as `members` for virtual workspaces
    // (`--workspace` is implied)
    // or only the root package for non-virtual workspaces.
    default_members: Vec<PathBuf>,

    // `true` if this is a temporary workspace created for the purposes of the
    // `cargo install` or `cargo package` commands.
    is_ephemeral: bool,

    // `true` if this workspace should enforce optional dependencies even when
    // not needed; false if this workspace should only enforce dependencies
    // needed by the current configuration (such as in cargo install). In some
    // cases `false` also results in the non-enforcement of dev-dependencies.
    require_optional_deps: bool,

    // A cache of loaded packages for particular paths which is disjoint from
    // `packages` up above, used in the `load` method down below.
    loaded_packages: RefCell<HashMap<PathBuf, Package>>,

    // If `true`, then the resolver will ignore any existing `Cargo.lock`
    // file. This is set for `cargo install` without `--locked`.
    ignore_lock: bool,
}

// Separate structure for tracking loaded packages (to avoid loading anything
// twice), and this is separate to help appease the borrow checker.
#[derive(Debug)]
struct Packages<'cfg> {
    config: &'cfg Config,
    packages: HashMap<PathBuf, MaybePackage>,
}

#[derive(Debug)]
enum MaybePackage {
    Package(Package),
    Virtual(VirtualManifest),
}

/// Configuration of a workspace in a manifest.
#[derive(Debug, Clone)]
pub enum WorkspaceConfig {
    /// Indicates that `[workspace]` was present and the members were
    /// optionally specified as well.
    Root(WorkspaceRootConfig),

    /// Indicates that `[workspace]` was present and the `root` field is the
    /// optional value of `package.workspace`, if present.
    Member { root: Option<String> },
}

/// Intermediate configuration of a workspace root in a manifest.
///
/// Knows the Workspace Root path, as well as `members` and `exclude` lists of path patterns, which
/// together tell if some path is recognized as a member by this root or not.
#[derive(Debug, Clone)]
pub struct WorkspaceRootConfig {
    root_dir: PathBuf,
    members: Option<Vec<String>>,
    default_members: Option<Vec<String>>,
    exclude: Vec<String>,
}

/// An iterator over the member packages of a workspace, returned by
/// `Workspace::members`
pub struct Members<'a, 'cfg> {
    ws: &'a Workspace<'cfg>,
    iter: slice::Iter<'a, PathBuf>,
}

impl<'cfg> Workspace<'cfg> {
    /// Creates a new workspace given the target manifest pointed to by
    /// `manifest_path`.
    ///
    /// This function will construct the entire workspace by determining the
    /// root and all member packages. It will then validate the workspace
    /// before returning it, so `Ok` is only returned for valid workspaces.
    pub fn new(manifest_path: &Path, config: &'cfg Config) -> CargoResult<Workspace<'cfg>> {
        let mut ws = Workspace::new_default(manifest_path.to_path_buf(), config);
        ws.target_dir = config.target_dir()?;
        ws.root_manifest = ws.find_root(manifest_path)?;
        ws.find_members()?;
        ws.validate()?;
        Ok(ws)
    }

    fn new_default(current_manifest: PathBuf, config: &'cfg Config) -> Workspace<'cfg> {
        Workspace {
            config,
            current_manifest,
            packages: Packages {
                config,
                packages: HashMap::new(),
            },
            root_manifest: None,
            target_dir: None,
            members: Vec::new(),
            member_ids: HashSet::new(),
            default_members: Vec::new(),
            is_ephemeral: false,
            require_optional_deps: true,
            loaded_packages: RefCell::new(HashMap::new()),
            ignore_lock: false,
        }
    }

    pub fn new_virtual(
        root_path: PathBuf,
        current_manifest: PathBuf,
        manifest: VirtualManifest,
        config: &'cfg Config,
    ) -> CargoResult<Workspace<'cfg>> {
        let mut ws = Workspace::new_default(current_manifest, config);
        ws.root_manifest = Some(root_path.join("Cargo.toml"));
        ws.target_dir = config.target_dir()?;
        ws.packages
            .packages
            .insert(root_path, MaybePackage::Virtual(manifest));
        ws.find_members()?;
        // TODO: validation does not work because it walks up the directory
        // tree looking for the root which is a fake file that doesn't exist.
        Ok(ws)
    }

    /// Creates a "temporary workspace" from one package which only contains
    /// that package.
    ///
    /// This constructor will not touch the filesystem and only creates an
    /// in-memory workspace. That is, all configuration is ignored, it's just
    /// intended for that one package.
    ///
    /// This is currently only used in niche situations like `cargo install` or
    /// `cargo package`.
    pub fn ephemeral(
        package: Package,
        config: &'cfg Config,
        target_dir: Option<Filesystem>,
        require_optional_deps: bool,
    ) -> CargoResult<Workspace<'cfg>> {
        let mut ws = Workspace::new_default(package.manifest_path().to_path_buf(), config);
        ws.is_ephemeral = true;
        ws.require_optional_deps = require_optional_deps;
        let key = ws.current_manifest.parent().unwrap();
        let id = package.package_id();
        let package = MaybePackage::Package(package);
        ws.packages.packages.insert(key.to_path_buf(), package);
        ws.target_dir = if let Some(dir) = target_dir {
            Some(dir)
        } else {
            ws.config.target_dir()?
        };
        ws.members.push(ws.current_manifest.clone());
        ws.member_ids.insert(id);
        ws.default_members.push(ws.current_manifest.clone());
        Ok(ws)
    }

    /// Returns the current package of this workspace.
    ///
    /// Note that this can return an error if it the current manifest is
    /// actually a "virtual Cargo.toml", in which case an error is returned
    /// indicating that something else should be passed.
    pub fn current(&self) -> CargoResult<&Package> {
        let pkg = self.current_opt().ok_or_else(|| {
            failure::format_err!(
                "manifest path `{}` is a virtual manifest, but this \
                 command requires running against an actual package in \
                 this workspace",
                self.current_manifest.display()
            )
        })?;
        Ok(pkg)
    }

    pub fn current_opt(&self) -> Option<&Package> {
        match *self.packages.get(&self.current_manifest) {
            MaybePackage::Package(ref p) => Some(p),
            MaybePackage::Virtual(..) => None,
        }
    }

    pub fn is_virtual(&self) -> bool {
        match *self.packages.get(&self.current_manifest) {
            MaybePackage::Package(..) => false,
            MaybePackage::Virtual(..) => true,
        }
    }

    /// Returns the `Config` this workspace is associated with.
    pub fn config(&self) -> &'cfg Config {
        self.config
    }

    pub fn profiles(&self) -> &Profiles {
        match self.root_maybe() {
            MaybePackage::Package(p) => p.manifest().profiles(),
            MaybePackage::Virtual(vm) => vm.profiles(),
        }
    }

    /// Returns the root path of this workspace.
    ///
    /// That is, this returns the path of the directory containing the
    /// `Cargo.toml` which is the root of this workspace.
    pub fn root(&self) -> &Path {
        match self.root_manifest {
            Some(ref p) => p,
            None => &self.current_manifest,
        }
        .parent()
        .unwrap()
    }

    /// Returns the root Package or VirtualManifest.
    fn root_maybe(&self) -> &MaybePackage {
        let root = self
            .root_manifest
            .as_ref()
            .unwrap_or(&self.current_manifest);
        self.packages.get(root)
    }

    pub fn target_dir(&self) -> Filesystem {
        self.target_dir
            .clone()
            .unwrap_or_else(|| Filesystem::new(self.root().join("target")))
    }

    /// Returns the root `[replace]` section of this workspace.
    ///
    /// This may be from a virtual crate or an actual crate.
    pub fn root_replace(&self) -> &[(PackageIdSpec, Dependency)] {
        match self.root_maybe() {
            MaybePackage::Package(p) => p.manifest().replace(),
            MaybePackage::Virtual(vm) => vm.replace(),
        }
    }

    /// Returns the root `[patch]` section of this workspace.
    ///
    /// This may be from a virtual crate or an actual crate.
    pub fn root_patch(&self) -> &HashMap<Url, Vec<Dependency>> {
        match self.root_maybe() {
            MaybePackage::Package(p) => p.manifest().patch(),
            MaybePackage::Virtual(vm) => vm.patch(),
        }
    }

    /// Returns an iterator over all packages in this workspace
    pub fn members<'a>(&'a self) -> Members<'a, 'cfg> {
        Members {
            ws: self,
            iter: self.members.iter(),
        }
    }

    /// Returns an iterator over default packages in this workspace
    pub fn default_members<'a>(&'a self) -> Members<'a, 'cfg> {
        Members {
            ws: self,
            iter: self.default_members.iter(),
        }
    }

    /// Returns true if the package is a member of the workspace.
    pub fn is_member(&self, pkg: &Package) -> bool {
        self.member_ids.contains(&pkg.package_id())
    }

    pub fn is_ephemeral(&self) -> bool {
        self.is_ephemeral
    }

    pub fn require_optional_deps(&self) -> bool {
        self.require_optional_deps
    }

    pub fn set_require_optional_deps(
        &mut self,
        require_optional_deps: bool,
    ) -> &mut Workspace<'cfg> {
        self.require_optional_deps = require_optional_deps;
        self
    }

    pub fn ignore_lock(&self) -> bool {
        self.ignore_lock
    }

    pub fn set_ignore_lock(&mut self, ignore_lock: bool) -> &mut Workspace<'cfg> {
        self.ignore_lock = ignore_lock;
        self
    }

    /// Finds the root of a workspace for the crate whose manifest is located
    /// at `manifest_path`.
    ///
    /// This will parse the `Cargo.toml` at `manifest_path` and then interpret
    /// the workspace configuration, optionally walking up the filesystem
    /// looking for other workspace roots.
    ///
    /// Returns an error if `manifest_path` isn't actually a valid manifest or
    /// if some other transient error happens.
    fn find_root(&mut self, manifest_path: &Path) -> CargoResult<Option<PathBuf>> {
        fn read_root_pointer(member_manifest: &Path, root_link: &str) -> CargoResult<PathBuf> {
            let path = member_manifest
                .parent()
                .unwrap()
                .join(root_link)
                .join("Cargo.toml");
            debug!("find_root - pointer {}", path.display());
            Ok(paths::normalize_path(&path))
        };

        {
            let current = self.packages.load(manifest_path)?;
            match *current.workspace_config() {
                WorkspaceConfig::Root(_) => {
                    debug!("find_root - is root {}", manifest_path.display());
                    return Ok(Some(manifest_path.to_path_buf()));
                }
                WorkspaceConfig::Member {
                    root: Some(ref path_to_root),
                } => return Ok(Some(read_root_pointer(manifest_path, path_to_root)?)),
                WorkspaceConfig::Member { root: None } => {}
            }
        }

        for path in paths::ancestors(manifest_path).skip(2) {
            if path.ends_with("target/package") {
                break;
            }

            let ances_manifest_path = path.join("Cargo.toml");
            debug!("find_root - trying {}", ances_manifest_path.display());
            if ances_manifest_path.exists() {
                match *self.packages.load(&ances_manifest_path)?.workspace_config() {
                    WorkspaceConfig::Root(ref ances_root_config) => {
                        debug!("find_root - found a root checking exclusion");
                        if !ances_root_config.is_excluded(manifest_path) {
                            debug!("find_root - found!");
                            return Ok(Some(ances_manifest_path));
                        }
                    }
                    WorkspaceConfig::Member {
                        root: Some(ref path_to_root),
                    } => {
                        debug!("find_root - found pointer");
                        return Ok(Some(read_root_pointer(&ances_manifest_path, path_to_root)?));
                    }
                    WorkspaceConfig::Member { .. } => {}
                }
            }

            // Don't walk across `CARGO_HOME` when we're looking for the
            // workspace root. Sometimes a package will be organized with
            // `CARGO_HOME` pointing inside of the workspace root or in the
            // current package, but we don't want to mistakenly try to put
            // crates.io crates into the workspace by accident.
            if self.config.home() == path {
                break;
            }
        }

        Ok(None)
    }

    /// After the root of a workspace has been located, probes for all members
    /// of a workspace.
    ///
    /// If the `workspace.members` configuration is present, then this just
    /// verifies that those are all valid packages to point to. Otherwise, this
    /// will transitively follow all `path` dependencies looking for members of
    /// the workspace.
    fn find_members(&mut self) -> CargoResult<()> {
        let root_manifest_path = match self.root_manifest {
            Some(ref path) => path.clone(),
            None => {
                debug!("find_members - only me as a member");
                self.members.push(self.current_manifest.clone());
                self.default_members.push(self.current_manifest.clone());
                if let Ok(pkg) = self.current() {
                    let id = pkg.package_id();
                    self.member_ids.insert(id);
                }
                return Ok(());
            }
        };

        let members_paths;
        let default_members_paths;
        {
            let root_package = self.packages.load(&root_manifest_path)?;
            match *root_package.workspace_config() {
                WorkspaceConfig::Root(ref root_config) => {
                    members_paths = root_config
                        .members_paths(root_config.members.as_ref().unwrap_or(&vec![]))?;
                    default_members_paths = if root_manifest_path == self.current_manifest {
                        if let Some(ref default) = root_config.default_members {
                            Some(root_config.members_paths(default)?)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                }
                _ => failure::bail!(
                    "root of a workspace inferred but wasn't a root: {}",
                    root_manifest_path.display()
                ),
            }
        }

        for path in members_paths {
            self.find_path_deps(&path.join("Cargo.toml"), &root_manifest_path, false)?;
        }

        if let Some(default) = default_members_paths {
            for path in default {
                let manifest_path = paths::normalize_path(&path.join("Cargo.toml"));
                if !self.members.contains(&manifest_path) {
                    failure::bail!(
                        "package `{}` is listed in workspace’s default-members \
                         but is not a member.",
                        path.display()
                    )
                }
                self.default_members.push(manifest_path)
            }
        } else if self.is_virtual() {
            self.default_members = self.members.clone()
        } else {
            self.default_members.push(self.current_manifest.clone())
        }

        self.find_path_deps(&root_manifest_path, &root_manifest_path, false)
    }

    fn find_path_deps(
        &mut self,
        manifest_path: &Path,
        root_manifest: &Path,
        is_path_dep: bool,
    ) -> CargoResult<()> {
        let manifest_path = paths::normalize_path(manifest_path);
        if self.members.contains(&manifest_path) {
            return Ok(());
        }
        if is_path_dep
            && !manifest_path.parent().unwrap().starts_with(self.root())
            && self.find_root(&manifest_path)? != self.root_manifest
        {
            // If `manifest_path` is a path dependency outside of the workspace,
            // don't add it, or any of its dependencies, as a members.
            return Ok(());
        }

        if let WorkspaceConfig::Root(ref root_config) =
            *self.packages.load(root_manifest)?.workspace_config()
        {
            if root_config.is_excluded(&manifest_path) {
                return Ok(());
            }
        }

        debug!("find_members - {}", manifest_path.display());
        self.members.push(manifest_path.clone());

        let candidates = {
            let pkg = match *self.packages.load(&manifest_path)? {
                MaybePackage::Package(ref p) => p,
                MaybePackage::Virtual(_) => return Ok(()),
            };
            self.member_ids.insert(pkg.package_id());
            pkg.dependencies()
                .iter()
                .map(|d| d.source_id())
                .filter(|d| d.is_path())
                .filter_map(|d| d.url().to_file_path().ok())
                .map(|p| p.join("Cargo.toml"))
                .collect::<Vec<_>>()
        };
        for candidate in candidates {
            self.find_path_deps(&candidate, root_manifest, true)
                .map_err(|err| ManifestError::new(err, manifest_path.clone()))?;
        }
        Ok(())
    }

    pub fn features(&self) -> &Features {
        match self.root_maybe() {
            MaybePackage::Package(p) => p.manifest().features(),
            MaybePackage::Virtual(vm) => vm.features(),
        }
    }

    /// Validates a workspace, ensuring that a number of invariants are upheld:
    ///
    /// 1. A workspace only has one root.
    /// 2. All workspace members agree on this one root as the root.
    /// 3. The current crate is a member of this workspace.
    fn validate(&mut self) -> CargoResult<()> {
        // Validate config profiles only once per workspace.
        let features = self.features();
        let mut warnings = Vec::new();
        self.config.profiles()?.validate(features, &mut warnings)?;
        for warning in warnings {
            self.config.shell().warn(&warning)?;
        }

        // The rest of the checks require a VirtualManifest or multiple members.
        if self.root_manifest.is_none() {
            return Ok(());
        }

        let mut roots = Vec::new();
        {
            let mut names = BTreeMap::new();
            for member in self.members.iter() {
                let package = self.packages.get(member);
                match *package.workspace_config() {
                    WorkspaceConfig::Root(_) => {
                        roots.push(member.parent().unwrap().to_path_buf());
                    }
                    WorkspaceConfig::Member { .. } => {}
                }
                let name = match *package {
                    MaybePackage::Package(ref p) => p.name(),
                    MaybePackage::Virtual(_) => continue,
                };
                if let Some(prev) = names.insert(name, member) {
                    failure::bail!(
                        "two packages named `{}` in this workspace:\n\
                         - {}\n\
                         - {}",
                        name,
                        prev.display(),
                        member.display()
                    );
                }
            }
        }

        match roots.len() {
            0 => failure::bail!(
                "`package.workspace` configuration points to a crate \
                 which is not configured with [workspace]: \n\
                 configuration at: {}\n\
                 points to: {}",
                self.current_manifest.display(),
                self.root_manifest.as_ref().unwrap().display()
            ),
            1 => {}
            _ => {
                failure::bail!(
                    "multiple workspace roots found in the same workspace:\n{}",
                    roots
                        .iter()
                        .map(|r| format!("  {}", r.display()))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }

        for member in self.members.clone() {
            let root = self.find_root(&member)?;
            if root == self.root_manifest {
                continue;
            }

            match root {
                Some(root) => {
                    failure::bail!(
                        "package `{}` is a member of the wrong workspace\n\
                         expected: {}\n\
                         actual:   {}",
                        member.display(),
                        self.root_manifest.as_ref().unwrap().display(),
                        root.display()
                    );
                }
                None => {
                    failure::bail!(
                        "workspace member `{}` is not hierarchically below \
                         the workspace root `{}`",
                        member.display(),
                        self.root_manifest.as_ref().unwrap().display()
                    );
                }
            }
        }

        if !self.members.contains(&self.current_manifest) {
            let root = self.root_manifest.as_ref().unwrap();
            let root_dir = root.parent().unwrap();
            let current_dir = self.current_manifest.parent().unwrap();
            let root_pkg = self.packages.get(root);

            // FIXME: Make this more generic by using a relative path resolver between member and
            // root.
            let members_msg = match current_dir.strip_prefix(root_dir) {
                Ok(rel) => format!(
                    "this may be fixable by adding `{}` to the \
                     `workspace.members` array of the manifest \
                     located at: {}",
                    rel.display(),
                    root.display()
                ),
                Err(_) => format!(
                    "this may be fixable by adding a member to \
                     the `workspace.members` array of the \
                     manifest located at: {}",
                    root.display()
                ),
            };
            let extra = match *root_pkg {
                MaybePackage::Virtual(_) => members_msg,
                MaybePackage::Package(ref p) => {
                    let has_members_list = match *p.manifest().workspace_config() {
                        WorkspaceConfig::Root(ref root_config) => root_config.has_members_list(),
                        WorkspaceConfig::Member { .. } => unreachable!(),
                    };
                    if !has_members_list {
                        format!(
                            "this may be fixable by ensuring that this \
                             crate is depended on by the workspace \
                             root: {}",
                            root.display()
                        )
                    } else {
                        members_msg
                    }
                }
            };
            failure::bail!(
                "current package believes it's in a workspace when it's not:\n\
                 current:   {}\n\
                 workspace: {}\n\n{}\n\
                 Alternatively, to keep it out of the workspace, add the package \
                 to the `workspace.exclude` array, or add an empty `[workspace]` \
                 table to the package's manifest.",
                self.current_manifest.display(),
                root.display(),
                extra
            );
        }

        if let Some(ref root_manifest) = self.root_manifest {
            for pkg in self
                .members()
                .filter(|p| p.manifest_path() != root_manifest)
            {
                let manifest = pkg.manifest();
                let emit_warning = |what| -> CargoResult<()> {
                    let msg = format!(
                        "{} for the non root package will be ignored, \
                         specify {} at the workspace root:\n\
                         package:   {}\n\
                         workspace: {}",
                        what,
                        what,
                        pkg.manifest_path().display(),
                        root_manifest.display(),
                    );
                    self.config.shell().warn(&msg)
                };
                if manifest.original().has_profiles() {
                    emit_warning("profiles")?;
                }
                if !manifest.replace().is_empty() {
                    emit_warning("replace")?;
                }
                if !manifest.patch().is_empty() {
                    emit_warning("patch")?;
                }
            }
        }

        Ok(())
    }

    pub fn load(&self, manifest_path: &Path) -> CargoResult<Package> {
        match self.packages.maybe_get(manifest_path) {
            Some(&MaybePackage::Package(ref p)) => return Ok(p.clone()),
            Some(&MaybePackage::Virtual(_)) => failure::bail!("cannot load workspace root"),
            None => {}
        }

        let mut loaded = self.loaded_packages.borrow_mut();
        if let Some(p) = loaded.get(manifest_path).cloned() {
            return Ok(p);
        }
        let source_id = SourceId::for_path(manifest_path.parent().unwrap())?;
        let (package, _nested_paths) = ops::read_package(manifest_path, source_id, self.config)?;
        loaded.insert(manifest_path.to_path_buf(), package.clone());
        Ok(package)
    }

    /// Preload the provided registry with already loaded packages.
    ///
    /// A workspace may load packages during construction/parsing/early phases
    /// for various operations, and this preload step avoids doubly-loading and
    /// parsing crates on the filesystem by inserting them all into the registry
    /// with their in-memory formats.
    pub fn preload(&self, registry: &mut PackageRegistry<'cfg>) {
        // These can get weird as this generally represents a workspace during
        // `cargo install`. Things like git repositories will actually have a
        // `PathSource` with multiple entries in it, so the logic below is
        // mostly just an optimization for normal `cargo build` in workspaces
        // during development.
        if self.is_ephemeral {
            return;
        }

        for pkg in self.packages.packages.values() {
            let pkg = match *pkg {
                MaybePackage::Package(ref p) => p.clone(),
                MaybePackage::Virtual(_) => continue,
            };
            let mut src = PathSource::new(
                pkg.manifest_path(),
                pkg.package_id().source_id(),
                self.config,
            );
            src.preload_with(pkg);
            registry.add_preloaded(Box::new(src));
        }
    }

    pub fn emit_warnings(&self) -> CargoResult<()> {
        for (path, maybe_pkg) in &self.packages.packages {
            let warnings = match maybe_pkg {
                MaybePackage::Package(pkg) => pkg.manifest().warnings().warnings(),
                MaybePackage::Virtual(vm) => vm.warnings().warnings(),
            };
            let path = path.join("Cargo.toml");
            for warning in warnings {
                if warning.is_critical {
                    let err = failure::format_err!("{}", warning.message);
                    let cx =
                        failure::format_err!("failed to parse manifest at `{}`", path.display());
                    return Err(err.context(cx).into());
                } else {
                    let msg = if self.root_manifest.is_none() {
                        warning.message.to_string()
                    } else {
                        // In a workspace, it can be confusing where a warning
                        // originated, so include the path.
                        format!("{}: {}", path.display(), warning.message)
                    };
                    self.config.shell().warn(msg)?
                }
            }
        }
        Ok(())
    }
}

impl<'cfg> Packages<'cfg> {
    fn get(&self, manifest_path: &Path) -> &MaybePackage {
        self.maybe_get(manifest_path).unwrap()
    }

    fn maybe_get(&self, manifest_path: &Path) -> Option<&MaybePackage> {
        self.packages.get(manifest_path.parent().unwrap())
    }

    fn load(&mut self, manifest_path: &Path) -> CargoResult<&MaybePackage> {
        let key = manifest_path.parent().unwrap();
        match self.packages.entry(key.to_path_buf()) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(v) => {
                let source_id = SourceId::for_path(key)?;
                let (manifest, _nested_paths) =
                    read_manifest(manifest_path, source_id, self.config)?;
                Ok(v.insert(match manifest {
                    EitherManifest::Real(manifest) => {
                        MaybePackage::Package(Package::new(manifest, manifest_path))
                    }
                    EitherManifest::Virtual(vm) => MaybePackage::Virtual(vm),
                }))
            }
        }
    }
}

impl<'a, 'cfg> Iterator for Members<'a, 'cfg> {
    type Item = &'a Package;

    fn next(&mut self) -> Option<&'a Package> {
        loop {
            let next = self.iter.next().map(|path| self.ws.packages.get(path));
            match next {
                Some(&MaybePackage::Package(ref p)) => return Some(p),
                Some(&MaybePackage::Virtual(_)) => {}
                None => return None,
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (_, upper) = self.iter.size_hint();
        (0, upper)
    }
}

impl MaybePackage {
    fn workspace_config(&self) -> &WorkspaceConfig {
        match *self {
            MaybePackage::Package(ref p) => p.manifest().workspace_config(),
            MaybePackage::Virtual(ref vm) => vm.workspace_config(),
        }
    }
}

impl WorkspaceRootConfig {
    /// Creates a new Intermediate Workspace Root configuration.
    pub fn new(
        root_dir: &Path,
        members: &Option<Vec<String>>,
        default_members: &Option<Vec<String>>,
        exclude: &Option<Vec<String>>,
    ) -> WorkspaceRootConfig {
        WorkspaceRootConfig {
            root_dir: root_dir.to_path_buf(),
            members: members.clone(),
            default_members: default_members.clone(),
            exclude: exclude.clone().unwrap_or_default(),
        }
    }

    /// Checks the path against the `excluded` list.
    ///
    /// This method does **not** consider the `members` list.
    fn is_excluded(&self, manifest_path: &Path) -> bool {
        let excluded = self
            .exclude
            .iter()
            .any(|ex| manifest_path.starts_with(self.root_dir.join(ex)));

        let explicit_member = match self.members {
            Some(ref members) => members
                .iter()
                .any(|mem| manifest_path.starts_with(self.root_dir.join(mem))),
            None => false,
        };

        !explicit_member && excluded
    }

    fn has_members_list(&self) -> bool {
        self.members.is_some()
    }

    fn members_paths(&self, globs: &[String]) -> CargoResult<Vec<PathBuf>> {
        let mut expanded_list = Vec::new();

        for glob in globs {
            let pathbuf = self.root_dir.join(glob);
            let expanded_paths = Self::expand_member_path(&pathbuf)?;

            // If glob does not find any valid paths, then put the original
            // path in the expanded list to maintain backwards compatibility.
            if expanded_paths.is_empty() {
                expanded_list.push(pathbuf);
            } else {
                expanded_list.extend(expanded_paths);
            }
        }

        Ok(expanded_list)
    }

    fn expand_member_path(path: &Path) -> CargoResult<Vec<PathBuf>> {
        let path = match path.to_str() {
            Some(p) => p,
            None => return Ok(Vec::new()),
        };
        let res =
            glob(path).chain_err(|| failure::format_err!("could not parse pattern `{}`", &path))?;
        let res = res
            .map(|p| {
                p.chain_err(|| failure::format_err!("unable to match path to pattern `{}`", &path))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(res)
    }
}
