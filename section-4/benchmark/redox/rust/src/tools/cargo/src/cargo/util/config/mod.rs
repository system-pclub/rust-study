use std::cell::{RefCell, RefMut};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::hash_map::HashMap;
use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, SeekFrom};
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Once;
use std::time::Instant;

use curl::easy::Easy;
use lazycell::LazyCell;
use serde::Deserialize;
use url::Url;

use self::ConfigValue as CV;
use crate::core::profiles::ConfigProfiles;
use crate::core::shell::Verbosity;
use crate::core::{nightly_features_allowed, CliUnstable, Shell, SourceId, Workspace};
use crate::ops;
use crate::util::errors::{self, internal, CargoResult, CargoResultExt};
use crate::util::toml as cargo_toml;
use crate::util::Filesystem;
use crate::util::Rustc;
use crate::util::{paths, validate_package_name, FileLock};
use crate::util::{IntoUrl, IntoUrlWithBase};

mod de;
use de::Deserializer;

mod value;
pub use value::{Definition, OptValue, Value};

mod key;
use key::ConfigKey;

mod path;
pub use path::ConfigRelativePath;

/// Configuration information for cargo. This is not specific to a build, it is information
/// relating to cargo itself.
///
/// This struct implements `Default`: all fields can be inferred.
#[derive(Debug)]
pub struct Config {
    /// The location of the user's 'home' directory. OS-dependent.
    home_path: Filesystem,
    /// Information about how to write messages to the shell
    shell: RefCell<Shell>,
    /// A collection of configuration options
    values: LazyCell<HashMap<String, ConfigValue>>,
    /// The current working directory of cargo
    cwd: PathBuf,
    /// The location of the cargo executable (path to current process)
    cargo_exe: LazyCell<PathBuf>,
    /// The location of the rustdoc executable
    rustdoc: LazyCell<PathBuf>,
    /// Whether we are printing extra verbose messages
    extra_verbose: bool,
    /// `frozen` is the same as `locked`, but additionally will not access the
    /// network to determine if the lock file is out-of-date.
    frozen: bool,
    /// `locked` is set if we should not update lock files. If the lock file
    /// is missing, or needs to be updated, an error is produced.
    locked: bool,
    /// `offline` is set if we should never access the network, but otherwise
    /// continue operating if possible.
    offline: bool,
    /// A global static IPC control mechanism (used for managing parallel builds)
    jobserver: Option<jobserver::Client>,
    /// Cli flags of the form "-Z something"
    cli_flags: CliUnstable,
    /// A handle on curl easy mode for http calls
    easy: LazyCell<RefCell<Easy>>,
    /// Cache of the `SourceId` for crates.io
    crates_io_source_id: LazyCell<SourceId>,
    /// If false, don't cache `rustc --version --verbose` invocations
    cache_rustc_info: bool,
    /// Creation time of this config, used to output the total build time
    creation_time: Instant,
    /// Target Directory via resolved Cli parameter
    target_dir: Option<Filesystem>,
    /// Environment variables, separated to assist testing.
    env: HashMap<String, String>,
    /// Profiles loaded from config.
    profiles: LazyCell<ConfigProfiles>,
    /// Tracks which sources have been updated to avoid multiple updates.
    updated_sources: LazyCell<RefCell<HashSet<SourceId>>>,
    /// Lock, if held, of the global package cache along with the number of
    /// acquisitions so far.
    package_cache_lock: RefCell<Option<(Option<FileLock>, usize)>>,
    /// Cached configuration parsed by Cargo
    http_config: LazyCell<CargoHttpConfig>,
    net_config: LazyCell<CargoNetConfig>,
    build_config: LazyCell<CargoBuildConfig>,
}

impl Config {
    pub fn new(shell: Shell, cwd: PathBuf, homedir: PathBuf) -> Config {
        static mut GLOBAL_JOBSERVER: *mut jobserver::Client = 0 as *mut _;
        static INIT: Once = Once::new();

        // This should be called early on in the process, so in theory the
        // unsafety is ok here. (taken ownership of random fds)
        INIT.call_once(|| unsafe {
            if let Some(client) = jobserver::Client::from_env() {
                GLOBAL_JOBSERVER = Box::into_raw(Box::new(client));
            }
        });

        let env: HashMap<_, _> = env::vars_os()
            .filter_map(|(k, v)| {
                // Ignore any key/values that are not valid Unicode.
                match (k.into_string(), v.into_string()) {
                    (Ok(k), Ok(v)) => Some((k, v)),
                    _ => None,
                }
            })
            .collect();

        let cache_rustc_info = match env.get("CARGO_CACHE_RUSTC_INFO") {
            Some(cache) => cache != "0",
            _ => true,
        };

        Config {
            home_path: Filesystem::new(homedir),
            shell: RefCell::new(shell),
            cwd,
            values: LazyCell::new(),
            cargo_exe: LazyCell::new(),
            rustdoc: LazyCell::new(),
            extra_verbose: false,
            frozen: false,
            locked: false,
            offline: false,
            jobserver: unsafe {
                if GLOBAL_JOBSERVER.is_null() {
                    None
                } else {
                    Some((*GLOBAL_JOBSERVER).clone())
                }
            },
            cli_flags: CliUnstable::default(),
            easy: LazyCell::new(),
            crates_io_source_id: LazyCell::new(),
            cache_rustc_info,
            creation_time: Instant::now(),
            target_dir: None,
            env,
            profiles: LazyCell::new(),
            updated_sources: LazyCell::new(),
            package_cache_lock: RefCell::new(None),
            http_config: LazyCell::new(),
            net_config: LazyCell::new(),
            build_config: LazyCell::new(),
        }
    }

    pub fn default() -> CargoResult<Config> {
        let shell = Shell::new();
        let cwd =
            env::current_dir().chain_err(|| "couldn't get the current directory of the process")?;
        let homedir = homedir(&cwd).ok_or_else(|| {
            failure::format_err!(
                "Cargo couldn't find your home directory. \
                 This probably means that $HOME was not set."
            )
        })?;
        Ok(Config::new(shell, cwd, homedir))
    }

    /// Gets the user's Cargo home directory (OS-dependent).
    pub fn home(&self) -> &Filesystem {
        &self.home_path
    }

    /// Gets the Cargo Git directory (`<cargo_home>/git`).
    pub fn git_path(&self) -> Filesystem {
        self.home_path.join("git")
    }

    /// Gets the Cargo registry index directory (`<cargo_home>/registry/index`).
    pub fn registry_index_path(&self) -> Filesystem {
        self.home_path.join("registry").join("index")
    }

    /// Gets the Cargo registry cache directory (`<cargo_home>/registry/path`).
    pub fn registry_cache_path(&self) -> Filesystem {
        self.home_path.join("registry").join("cache")
    }

    /// Gets the Cargo registry source directory (`<cargo_home>/registry/src`).
    pub fn registry_source_path(&self) -> Filesystem {
        self.home_path.join("registry").join("src")
    }

    /// Gets the default Cargo registry.
    pub fn default_registry(&self) -> CargoResult<Option<String>> {
        Ok(match self.get_string("registry.default")? {
            Some(registry) => Some(registry.val),
            None => None,
        })
    }

    /// Gets a reference to the shell, e.g., for writing error messages.
    pub fn shell(&self) -> RefMut<'_, Shell> {
        self.shell.borrow_mut()
    }

    /// Gets the path to the `rustdoc` executable.
    pub fn rustdoc(&self) -> CargoResult<&Path> {
        self.rustdoc
            .try_borrow_with(|| Ok(self.get_tool("rustdoc", &self.build_config()?.rustdoc)))
            .map(AsRef::as_ref)
    }

    /// Gets the path to the `rustc` executable.
    pub fn load_global_rustc(&self, ws: Option<&Workspace<'_>>) -> CargoResult<Rustc> {
        let cache_location = ws.map(|ws| {
            ws.target_dir()
                .join(".rustc_info.json")
                .into_path_unlocked()
        });
        let wrapper = self.maybe_get_tool("rustc_wrapper", &self.build_config()?.rustc_wrapper);
        Rustc::new(
            self.get_tool("rustc", &self.build_config()?.rustc),
            wrapper,
            &self
                .home()
                .join("bin")
                .join("rustc")
                .into_path_unlocked()
                .with_extension(env::consts::EXE_EXTENSION),
            if self.cache_rustc_info {
                cache_location
            } else {
                None
            },
        )
    }

    /// Gets the path to the `cargo` executable.
    pub fn cargo_exe(&self) -> CargoResult<&Path> {
        self.cargo_exe
            .try_borrow_with(|| {
                fn from_current_exe() -> CargoResult<PathBuf> {
                    // Try fetching the path to `cargo` using `env::current_exe()`.
                    // The method varies per operating system and might fail; in particular,
                    // it depends on `/proc` being mounted on Linux, and some environments
                    // (like containers or chroots) may not have that available.
                    let exe = env::current_exe()?.canonicalize()?;
                    Ok(exe)
                }

                fn from_argv() -> CargoResult<PathBuf> {
                    // Grab `argv[0]` and attempt to resolve it to an absolute path.
                    // If `argv[0]` has one component, it must have come from a `PATH` lookup,
                    // so probe `PATH` in that case.
                    // Otherwise, it has multiple components and is either:
                    // - a relative path (e.g., `./cargo`, `target/debug/cargo`), or
                    // - an absolute path (e.g., `/usr/local/bin/cargo`).
                    // In either case, `Path::canonicalize` will return the full absolute path
                    // to the target if it exists.
                    let argv0 = env::args_os()
                        .map(PathBuf::from)
                        .next()
                        .ok_or_else(|| failure::format_err!("no argv[0]"))?;
                    paths::resolve_executable(&argv0)
                }

                let exe = from_current_exe()
                    .or_else(|_| from_argv())
                    .chain_err(|| "couldn't get the path to cargo executable")?;
                Ok(exe)
            })
            .map(AsRef::as_ref)
    }

    pub fn profiles(&self) -> CargoResult<&ConfigProfiles> {
        self.profiles.try_borrow_with(|| {
            let ocp = self.get::<Option<ConfigProfiles>>("profile")?;
            if let Some(config_profiles) = ocp {
                // Warn if config profiles without CLI option.
                if !self.cli_unstable().config_profile {
                    self.shell().warn(
                        "profiles in config files require `-Z config-profile` \
                         command-line option",
                    )?;
                    return Ok(ConfigProfiles::default());
                }
                Ok(config_profiles)
            } else {
                Ok(ConfigProfiles::default())
            }
        })
    }

    pub fn updated_sources(&self) -> RefMut<'_, HashSet<SourceId>> {
        self.updated_sources
            .borrow_with(|| RefCell::new(HashSet::new()))
            .borrow_mut()
    }

    pub fn values(&self) -> CargoResult<&HashMap<String, ConfigValue>> {
        self.values.try_borrow_with(|| self.load_values())
    }

    pub fn values_mut(&mut self) -> CargoResult<&mut HashMap<String, ConfigValue>> {
        match self.values.borrow_mut() {
            Some(map) => Ok(map),
            None => failure::bail!("config values not loaded yet"),
        }
    }

    // Note: this is used by RLS, not Cargo.
    pub fn set_values(&self, values: HashMap<String, ConfigValue>) -> CargoResult<()> {
        if self.values.borrow().is_some() {
            failure::bail!("config values already found")
        }
        match self.values.fill(values) {
            Ok(()) => Ok(()),
            Err(_) => failure::bail!("could not fill values"),
        }
    }

    pub fn reload_rooted_at<P: AsRef<Path>>(&mut self, path: P) -> CargoResult<()> {
        let values = self.load_values_from(path.as_ref())?;
        self.values.replace(values);
        Ok(())
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn target_dir(&self) -> CargoResult<Option<Filesystem>> {
        if let Some(dir) = &self.target_dir {
            Ok(Some(dir.clone()))
        } else if let Some(dir) = env::var_os("CARGO_TARGET_DIR") {
            Ok(Some(Filesystem::new(self.cwd.join(dir))))
        } else if let Some(val) = &self.build_config()?.target_dir {
            let val = val.resolve_path(self);
            Ok(Some(Filesystem::new(val)))
        } else {
            Ok(None)
        }
    }

    fn get_cv(&self, key: &str) -> CargoResult<Option<ConfigValue>> {
        let vals = self.values()?;
        let mut parts = key.split('.').enumerate();
        let mut val = match vals.get(parts.next().unwrap().1) {
            Some(val) => val,
            None => return Ok(None),
        };
        for (i, part) in parts {
            match *val {
                CV::Table(ref map, _) => {
                    val = match map.get(part) {
                        Some(val) => val,
                        None => return Ok(None),
                    }
                }
                CV::Integer(_, ref path)
                | CV::String(_, ref path)
                | CV::List(_, ref path)
                | CV::Boolean(_, ref path) => {
                    let idx = key.split('.').take(i).fold(0, |n, s| n + s.len()) + i - 1;
                    let key_so_far = &key[..idx];
                    failure::bail!(
                        "expected table for configuration key `{}`, \
                         but found {} in {}",
                        key_so_far,
                        val.desc(),
                        path.display()
                    )
                }
            }
        }
        Ok(Some(val.clone()))
    }

    // Helper primarily for testing.
    pub fn set_env(&mut self, env: HashMap<String, String>) {
        self.env = env;
    }

    fn get_env<T>(&self, key: &ConfigKey) -> Result<OptValue<T>, ConfigError>
    where
        T: FromStr,
        <T as FromStr>::Err: fmt::Display,
    {
        match self.env.get(key.as_env_key()) {
            Some(value) => {
                let definition = Definition::Environment(key.as_env_key().to_string());
                Ok(Some(Value {
                    val: value
                        .parse()
                        .map_err(|e| ConfigError::new(format!("{}", e), definition.clone()))?,
                    definition,
                }))
            }
            None => Ok(None),
        }
    }

    fn has_key(&self, key: &ConfigKey) -> bool {
        if self.env.get(key.as_env_key()).is_some() {
            return true;
        }
        let env_pattern = format!("{}_", key.as_env_key());
        if self.env.keys().any(|k| k.starts_with(&env_pattern)) {
            return true;
        }
        if let Ok(o_cv) = self.get_cv(key.as_config_key()) {
            if o_cv.is_some() {
                return true;
            }
        }
        false
    }

    pub fn get_string(&self, key: &str) -> CargoResult<OptValue<String>> {
        self.get::<Option<Value<String>>>(key)
    }

    fn get_string_priv(&self, key: &ConfigKey) -> Result<OptValue<String>, ConfigError> {
        match self.get_env(key)? {
            Some(v) => Ok(Some(v)),
            None => {
                let o_cv = self.get_cv(key.as_config_key())?;
                match o_cv {
                    Some(CV::String(s, path)) => Ok(Some(Value {
                        val: s,
                        definition: Definition::Path(path),
                    })),
                    Some(cv) => Err(ConfigError::expected(key.as_config_key(), "a string", &cv)),
                    None => Ok(None),
                }
            }
        }
    }

    fn get_bool_priv(&self, key: &ConfigKey) -> Result<OptValue<bool>, ConfigError> {
        match self.get_env(key)? {
            Some(v) => Ok(Some(v)),
            None => {
                let o_cv = self.get_cv(key.as_config_key())?;
                match o_cv {
                    Some(CV::Boolean(b, path)) => Ok(Some(Value {
                        val: b,
                        definition: Definition::Path(path),
                    })),
                    Some(cv) => Err(ConfigError::expected(
                        key.as_config_key(),
                        "true/false",
                        &cv,
                    )),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn get_path(&self, key: &str) -> CargoResult<OptValue<PathBuf>> {
        self.get::<Option<Value<ConfigRelativePath>>>(key).map(|v| {
            v.map(|v| Value {
                val: v.val.resolve_program(self),
                definition: v.definition,
            })
        })
    }

    fn string_to_path(&self, value: String, definition: &Definition) -> PathBuf {
        let is_path = value.contains('/') || (cfg!(windows) && value.contains('\\'));
        if is_path {
            definition.root(self).join(value)
        } else {
            // A pathless name.
            PathBuf::from(value)
        }
    }

    pub fn get_path_and_args(&self, key: &str) -> CargoResult<OptValue<(PathBuf, Vec<String>)>> {
        if let Some(mut val) = self.get_list_or_split_string(key)? {
            if !val.val.is_empty() {
                return Ok(Some(Value {
                    val: (
                        self.string_to_path(val.val.remove(0), &val.definition),
                        val.val,
                    ),
                    definition: val.definition,
                }));
            }
        }
        Ok(None)
    }

    // NOTE: this does **not** support environment variables. Use `get` instead
    // if you want that.
    pub fn get_list(&self, key: &str) -> CargoResult<OptValue<Vec<(String, PathBuf)>>> {
        match self.get_cv(key)? {
            Some(CV::List(i, path)) => Ok(Some(Value {
                val: i,
                definition: Definition::Path(path),
            })),
            Some(val) => self.expected("list", key, &val),
            None => Ok(None),
        }
    }

    fn get_list_or_split_string(&self, key: &str) -> CargoResult<OptValue<Vec<String>>> {
        match self.get::<Option<Value<StringList>>>(key)? {
            None => Ok(None),
            Some(val) => Ok(Some(Value {
                val: val.val.list,
                definition: val.definition,
            })),
        }
    }

    pub fn get_table(&self, key: &str) -> CargoResult<OptValue<HashMap<String, CV>>> {
        match self.get_cv(key)? {
            Some(CV::Table(i, path)) => Ok(Some(Value {
                val: i,
                definition: Definition::Path(path),
            })),
            Some(val) => self.expected("table", key, &val),
            None => Ok(None),
        }
    }

    fn get_integer(&self, key: &ConfigKey) -> Result<OptValue<i64>, ConfigError> {
        match self.get_env::<i64>(key)? {
            Some(v) => Ok(Some(v)),
            None => match self.get_cv(key.as_config_key())? {
                Some(CV::Integer(i, path)) => Ok(Some(Value {
                    val: i,
                    definition: Definition::Path(path),
                })),
                Some(cv) => Err(ConfigError::expected(
                    key.as_config_key(),
                    "an integer",
                    &cv,
                )),
                None => Ok(None),
            },
        }
    }

    fn expected<T>(&self, ty: &str, key: &str, val: &CV) -> CargoResult<T> {
        val.expected(ty, key)
            .map_err(|e| failure::format_err!("invalid configuration for key `{}`\n{}", key, e))
    }

    pub fn configure(
        &mut self,
        verbose: u32,
        quiet: Option<bool>,
        color: &Option<String>,
        frozen: bool,
        locked: bool,
        offline: bool,
        target_dir: &Option<PathBuf>,
        unstable_flags: &[String],
    ) -> CargoResult<()> {
        let extra_verbose = verbose >= 2;
        let verbose = if verbose == 0 { None } else { Some(true) };

        #[derive(Deserialize, Default)]
        struct TermConfig {
            verbose: Option<bool>,
            color: Option<String>,
        }

        // Ignore errors in the configuration files.
        let cfg = self.get::<TermConfig>("term").unwrap_or_default();

        let color = color.as_ref().or_else(|| cfg.color.as_ref());

        let verbosity = match (verbose, cfg.verbose, quiet) {
            (Some(true), _, None) | (None, Some(true), None) => Verbosity::Verbose,

            // Command line takes precedence over configuration, so ignore the
            // configuration..
            (None, _, Some(true)) => Verbosity::Quiet,

            // Can't pass both at the same time on the command line regardless
            // of configuration.
            (Some(true), _, Some(true)) => {
                failure::bail!("cannot set both --verbose and --quiet");
            }

            // Can't actually get `Some(false)` as a value from the command
            // line, so just ignore them here to appease exhaustiveness checking
            // in match statements.
            (Some(false), _, _)
            | (_, _, Some(false))
            | (None, Some(false), None)
            | (None, None, None) => Verbosity::Normal,
        };

        let cli_target_dir = match target_dir.as_ref() {
            Some(dir) => Some(Filesystem::new(dir.clone())),
            None => None,
        };

        self.shell().set_verbosity(verbosity);
        self.shell().set_color_choice(color.map(|s| &s[..]))?;
        self.extra_verbose = extra_verbose;
        self.frozen = frozen;
        self.locked = locked;
        self.offline = offline
            || self
                .net_config()
                .ok()
                .and_then(|n| n.offline)
                .unwrap_or(false);
        self.target_dir = cli_target_dir;
        self.cli_flags.parse(unstable_flags)?;

        if nightly_features_allowed() {
            if let Some(val) = self.get::<Option<bool>>("unstable.mtime_on_use")? {
                self.cli_flags.mtime_on_use |= val;
            }
        }

        Ok(())
    }

    pub fn cli_unstable(&self) -> &CliUnstable {
        &self.cli_flags
    }

    pub fn extra_verbose(&self) -> bool {
        self.extra_verbose
    }

    pub fn network_allowed(&self) -> bool {
        !self.frozen() && !self.offline()
    }

    pub fn offline(&self) -> bool {
        self.offline
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    pub fn lock_update_allowed(&self) -> bool {
        !self.frozen && !self.locked
    }

    /// Loads configuration from the filesystem.
    pub fn load_values(&self) -> CargoResult<HashMap<String, ConfigValue>> {
        self.load_values_from(&self.cwd)
    }

    fn load_values_from(&self, path: &Path) -> CargoResult<HashMap<String, ConfigValue>> {
        let mut cfg = CV::Table(HashMap::new(), PathBuf::from("."));
        let home = self.home_path.clone().into_path_unlocked();

        self.walk_tree(path, &home, |path| {
            let mut contents = String::new();
            let mut file = File::open(&path)?;
            file.read_to_string(&mut contents)
                .chain_err(|| format!("failed to read configuration file `{}`", path.display()))?;
            let toml = cargo_toml::parse(&contents, path, self).chain_err(|| {
                format!("could not parse TOML configuration in `{}`", path.display())
            })?;
            let value = CV::from_toml(path, toml).chain_err(|| {
                format!(
                    "failed to load TOML configuration from `{}`",
                    path.display()
                )
            })?;
            cfg.merge(value)
                .chain_err(|| format!("failed to merge configuration at `{}`", path.display()))?;
            Ok(())
        })
        .chain_err(|| "could not load Cargo configuration")?;

        self.load_credentials(&mut cfg)?;
        match cfg {
            CV::Table(map, _) => Ok(map),
            _ => unreachable!(),
        }
    }

    /// The purpose of this function is to aid in the transition to using
    /// .toml extensions on Cargo's config files, which were historically not used.
    /// Both 'config.toml' and 'credentials.toml' should be valid with or without extension.
    /// When both exist, we want to prefer the one without an extension for
    /// backwards compatibility, but warn the user appropriately.
    fn get_file_path(
        &self,
        dir: &Path,
        filename_without_extension: &str,
        warn: bool,
    ) -> CargoResult<Option<PathBuf>> {
        let possible = dir.join(filename_without_extension);
        let possible_with_extension = dir.join(format!("{}.toml", filename_without_extension));

        if fs::metadata(&possible).is_ok() {
            if warn && fs::metadata(&possible_with_extension).is_ok() {
                // We don't want to print a warning if the version
                // without the extension is just a symlink to the version
                // WITH an extension, which people may want to do to
                // support multiple Cargo versions at once and not
                // get a warning.
                let skip_warning = if let Ok(target_path) = fs::read_link(&possible) {
                    target_path == possible_with_extension
                } else {
                    false
                };

                if !skip_warning {
                    self.shell().warn(format!(
                        "Both `{}` and `{}` exist. Using `{}`",
                        possible.display(),
                        possible_with_extension.display(),
                        possible.display()
                    ))?;
                }
            }

            Ok(Some(possible))
        } else if fs::metadata(&possible_with_extension).is_ok() {
            Ok(Some(possible_with_extension))
        } else {
            Ok(None)
        }
    }

    fn walk_tree<F>(&self, pwd: &Path, home: &Path, mut walk: F) -> CargoResult<()>
    where
        F: FnMut(&Path) -> CargoResult<()>,
    {
        let mut stash: HashSet<PathBuf> = HashSet::new();

        for current in paths::ancestors(pwd) {
            if let Some(path) = self.get_file_path(&current.join(".cargo"), "config", true)? {
                walk(&path)?;
                stash.insert(path);
            }
        }

        // Once we're done, also be sure to walk the home directory even if it's not
        // in our history to be sure we pick up that standard location for
        // information.
        if let Some(path) = self.get_file_path(home, "config", true)? {
            if !stash.contains(&path) {
                walk(&path)?;
            }
        }

        Ok(())
    }

    /// Gets the index for a registry.
    pub fn get_registry_index(&self, registry: &str) -> CargoResult<Url> {
        validate_package_name(registry, "registry name", "")?;
        Ok(
            match self.get_string(&format!("registries.{}.index", registry))? {
                Some(index) => self.resolve_registry_index(index)?,
                None => failure::bail!("No index found for registry: `{}`", registry),
            },
        )
    }

    /// Gets the index for the default registry.
    pub fn get_default_registry_index(&self) -> CargoResult<Option<Url>> {
        Ok(match self.get_string("registry.index")? {
            Some(index) => Some(self.resolve_registry_index(index)?),
            None => None,
        })
    }

    fn resolve_registry_index(&self, index: Value<String>) -> CargoResult<Url> {
        let base = index
            .definition
            .root(self)
            .join("truncated-by-url_with_base");
        // Parse val to check it is a URL, not a relative path without a protocol.
        let _parsed = index.val.into_url()?;
        let url = index.val.into_url_with_base(Some(&*base))?;
        if url.password().is_some() {
            failure::bail!("Registry URLs may not contain passwords");
        }
        Ok(url)
    }

    /// Loads credentials config from the credentials file into the `ConfigValue` object, if
    /// present.
    fn load_credentials(&self, cfg: &mut ConfigValue) -> CargoResult<()> {
        let home_path = self.home_path.clone().into_path_unlocked();
        let credentials = match self.get_file_path(&home_path, "credentials", true)? {
            Some(credentials) => credentials,
            None => return Ok(()),
        };

        let mut contents = String::new();
        let mut file = File::open(&credentials)?;
        file.read_to_string(&mut contents).chain_err(|| {
            format!(
                "failed to read configuration file `{}`",
                credentials.display()
            )
        })?;

        let toml = cargo_toml::parse(&contents, &credentials, self).chain_err(|| {
            format!(
                "could not parse TOML configuration in `{}`",
                credentials.display()
            )
        })?;

        let mut value = CV::from_toml(&credentials, toml).chain_err(|| {
            format!(
                "failed to load TOML configuration from `{}`",
                credentials.display()
            )
        })?;

        // Backwards compatibility for old `.cargo/credentials` layout.
        {
            let value = match value {
                CV::Table(ref mut value, _) => value,
                _ => unreachable!(),
            };

            if let Some(token) = value.remove("token") {
                if let Vacant(entry) = value.entry("registry".into()) {
                    let mut map = HashMap::new();
                    map.insert("token".into(), token);
                    let table = CV::Table(map, PathBuf::from("."));
                    entry.insert(table);
                }
            }
        }

        // We want value to override `cfg`, so swap these.
        mem::swap(cfg, &mut value);
        cfg.merge(value)?;

        Ok(())
    }

    /// Looks for a path for `tool` in an environment variable or the given config, and returns
    /// `None` if it's not present.
    fn maybe_get_tool(&self, tool: &str, from_config: &Option<PathBuf>) -> Option<PathBuf> {
        let var = tool.to_uppercase();

        match env::var_os(&var) {
            Some(tool_path) => {
                let maybe_relative = match tool_path.to_str() {
                    Some(s) => s.contains('/') || s.contains('\\'),
                    None => false,
                };
                let path = if maybe_relative {
                    self.cwd.join(tool_path)
                } else {
                    PathBuf::from(tool_path)
                };
                Some(path)
            }

            None => from_config.clone(),
        }
    }

    /// Looks for a path for `tool` in an environment variable or config path, defaulting to `tool`
    /// as a path.
    fn get_tool(&self, tool: &str, from_config: &Option<PathBuf>) -> PathBuf {
        self.maybe_get_tool(tool, from_config)
            .unwrap_or_else(|| PathBuf::from(tool))
    }

    pub fn jobserver_from_env(&self) -> Option<&jobserver::Client> {
        self.jobserver.as_ref()
    }

    pub fn http(&self) -> CargoResult<&RefCell<Easy>> {
        let http = self
            .easy
            .try_borrow_with(|| ops::http_handle(self).map(RefCell::new))?;
        {
            let mut http = http.borrow_mut();
            http.reset();
            let timeout = ops::configure_http_handle(self, &mut http)?;
            timeout.configure(&mut http)?;
        }
        Ok(http)
    }

    pub fn http_config(&self) -> CargoResult<&CargoHttpConfig> {
        self.http_config
            .try_borrow_with(|| Ok(self.get::<CargoHttpConfig>("http")?))
    }

    pub fn net_config(&self) -> CargoResult<&CargoNetConfig> {
        self.net_config
            .try_borrow_with(|| Ok(self.get::<CargoNetConfig>("net")?))
    }

    pub fn build_config(&self) -> CargoResult<&CargoBuildConfig> {
        self.build_config
            .try_borrow_with(|| Ok(self.get::<CargoBuildConfig>("build")?))
    }

    pub fn crates_io_source_id<F>(&self, f: F) -> CargoResult<SourceId>
    where
        F: FnMut() -> CargoResult<SourceId>,
    {
        Ok(*(self.crates_io_source_id.try_borrow_with(f)?))
    }

    pub fn creation_time(&self) -> Instant {
        self.creation_time
    }

    // Retrieves a config variable.
    //
    // This supports most serde `Deserialize` types. Examples:
    //
    //     let v: Option<u32> = config.get("some.nested.key")?;
    //     let v: Option<MyStruct> = config.get("some.key")?;
    //     let v: Option<HashMap<String, MyStruct>> = config.get("foo")?;
    pub fn get<'de, T: serde::de::Deserialize<'de>>(&self, key: &str) -> CargoResult<T> {
        let d = Deserializer {
            config: self,
            key: ConfigKey::from_str(key),
        };
        T::deserialize(d).map_err(|e| e.into())
    }

    pub fn assert_package_cache_locked<'a>(&self, f: &'a Filesystem) -> &'a Path {
        let ret = f.as_path_unlocked();
        assert!(
            self.package_cache_lock.borrow().is_some(),
            "package cache lock is not currently held, Cargo forgot to call \
             `acquire_package_cache_lock` before we got to this stack frame",
        );
        assert!(ret.starts_with(self.home_path.as_path_unlocked()));
        ret
    }

    /// Acquires an exclusive lock on the global "package cache"
    ///
    /// This lock is global per-process and can be acquired recursively. An RAII
    /// structure is returned to release the lock, and if this process
    /// abnormally terminates the lock is also released.
    pub fn acquire_package_cache_lock(&self) -> CargoResult<PackageCacheLock<'_>> {
        let mut slot = self.package_cache_lock.borrow_mut();
        match *slot {
            // We've already acquired the lock in this process, so simply bump
            // the count and continue.
            Some((_, ref mut cnt)) => {
                *cnt += 1;
            }
            None => {
                let path = ".package-cache";
                let desc = "package cache";

                // First, attempt to open an exclusive lock which is in general
                // the purpose of this lock!
                //
                // If that fails because of a readonly filesystem or a
                // permission error, though, then we don't really want to fail
                // just because of this. All files that this lock protects are
                // in subfolders, so they're assumed by Cargo to also be
                // readonly or have invalid permissions for us to write to. If
                // that's the case, then we don't really need to grab a lock in
                // the first place here.
                //
                // Despite this we attempt to grab a readonly lock. This means
                // that if our read-only folder is shared read-write with
                // someone else on the system we should synchronize with them,
                // but if we can't even do that then we did our best and we just
                // keep on chugging elsewhere.
                match self.home_path.open_rw(path, self, desc) {
                    Ok(lock) => *slot = Some((Some(lock), 1)),
                    Err(e) => {
                        if maybe_readonly(&e) {
                            let lock = self.home_path.open_ro(path, self, desc).ok();
                            *slot = Some((lock, 1));
                            return Ok(PackageCacheLock(self));
                        }

                        Err(e).chain_err(|| "failed to acquire package cache lock")?;
                    }
                }
            }
        }
        return Ok(PackageCacheLock(self));

        fn maybe_readonly(err: &failure::Error) -> bool {
            err.iter_chain().any(|err| {
                if let Some(io) = err.downcast_ref::<io::Error>() {
                    if io.kind() == io::ErrorKind::PermissionDenied {
                        return true;
                    }

                    #[cfg(unix)]
                    return io.raw_os_error() == Some(libc::EROFS);
                }

                false
            })
        }
    }

    pub fn release_package_cache_lock(&self) {}
}

/// Internal error for serde errors.
#[derive(Debug)]
pub struct ConfigError {
    error: failure::Error,
    definition: Option<Definition>,
}

impl ConfigError {
    fn new(message: String, definition: Definition) -> ConfigError {
        ConfigError {
            error: failure::err_msg(message),
            definition: Some(definition),
        }
    }

    fn expected(key: &str, expected: &str, found: &ConfigValue) -> ConfigError {
        ConfigError {
            error: failure::format_err!(
                "`{}` expected {}, but found a {}",
                key,
                expected,
                found.desc()
            ),
            definition: Some(Definition::Path(found.definition_path().to_path_buf())),
        }
    }

    fn missing(key: &ConfigKey) -> ConfigError {
        ConfigError {
            error: failure::format_err!("missing config key `{}`", key.as_config_key()),
            definition: None,
        }
    }

    fn with_key_context(self, key: &ConfigKey, definition: Definition) -> ConfigError {
        ConfigError {
            error: failure::format_err!(
                "could not load config key `{}`: {}",
                key.as_config_key(),
                self
            ),
            definition: Some(definition),
        }
    }
}

impl std::error::Error for ConfigError {}

// Future note: currently, we cannot override `Fail::cause` (due to
// specialization) so we have no way to return the underlying causes. In the
// future, once this limitation is lifted, this should instead implement
// `cause` and avoid doing the cause formatting here.
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = errors::display_causes(&self.error);
        if let Some(ref definition) = self.definition {
            write!(f, "error in {}: {}", definition, message)
        } else {
            message.fmt(f)
        }
    }
}

impl serde::de::Error for ConfigError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        ConfigError {
            error: failure::err_msg(msg.to_string()),
            definition: None,
        }
    }
}

impl From<failure::Error> for ConfigError {
    fn from(error: failure::Error) -> Self {
        ConfigError {
            error,
            definition: None,
        }
    }
}

#[derive(Eq, PartialEq, Clone)]
pub enum ConfigValue {
    Integer(i64, PathBuf),
    String(String, PathBuf),
    List(Vec<(String, PathBuf)>, PathBuf),
    Table(HashMap<String, ConfigValue>, PathBuf),
    Boolean(bool, PathBuf),
}

impl fmt::Debug for ConfigValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            CV::Integer(i, ref path) => write!(f, "{} (from {})", i, path.display()),
            CV::Boolean(b, ref path) => write!(f, "{} (from {})", b, path.display()),
            CV::String(ref s, ref path) => write!(f, "{} (from {})", s, path.display()),
            CV::List(ref list, ref path) => {
                write!(f, "[")?;
                for (i, &(ref s, ref path)) in list.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} (from {})", s, path.display())?;
                }
                write!(f, "] (from {})", path.display())
            }
            CV::Table(ref table, _) => write!(f, "{:?}", table),
        }
    }
}

impl ConfigValue {
    fn from_toml(path: &Path, toml: toml::Value) -> CargoResult<ConfigValue> {
        match toml {
            toml::Value::String(val) => Ok(CV::String(val, path.to_path_buf())),
            toml::Value::Boolean(b) => Ok(CV::Boolean(b, path.to_path_buf())),
            toml::Value::Integer(i) => Ok(CV::Integer(i, path.to_path_buf())),
            toml::Value::Array(val) => Ok(CV::List(
                val.into_iter()
                    .map(|toml| match toml {
                        toml::Value::String(val) => Ok((val, path.to_path_buf())),
                        v => failure::bail!("expected string but found {} in list", v.type_str()),
                    })
                    .collect::<CargoResult<_>>()?,
                path.to_path_buf(),
            )),
            toml::Value::Table(val) => Ok(CV::Table(
                val.into_iter()
                    .map(|(key, value)| {
                        let value = CV::from_toml(path, value)
                            .chain_err(|| format!("failed to parse key `{}`", key))?;
                        Ok((key, value))
                    })
                    .collect::<CargoResult<_>>()?,
                path.to_path_buf(),
            )),
            v => failure::bail!(
                "found TOML configuration value of unknown type `{}`",
                v.type_str()
            ),
        }
    }

    fn into_toml(self) -> toml::Value {
        match self {
            CV::Boolean(s, _) => toml::Value::Boolean(s),
            CV::String(s, _) => toml::Value::String(s),
            CV::Integer(i, _) => toml::Value::Integer(i),
            CV::List(l, _) => {
                toml::Value::Array(l.into_iter().map(|(s, _)| toml::Value::String(s)).collect())
            }
            CV::Table(l, _) => {
                toml::Value::Table(l.into_iter().map(|(k, v)| (k, v.into_toml())).collect())
            }
        }
    }

    fn merge(&mut self, from: ConfigValue) -> CargoResult<()> {
        match (self, from) {
            (&mut CV::List(ref mut old, _), CV::List(ref mut new, _)) => {
                let new = mem::replace(new, Vec::new());
                old.extend(new.into_iter());
            }
            (&mut CV::Table(ref mut old, _), CV::Table(ref mut new, _)) => {
                let new = mem::replace(new, HashMap::new());
                for (key, value) in new {
                    match old.entry(key.clone()) {
                        Occupied(mut entry) => {
                            let path = value.definition_path().to_path_buf();
                            let entry = entry.get_mut();
                            entry.merge(value).chain_err(|| {
                                format!(
                                    "failed to merge key `{}` between \
                                     files:\n  \
                                     file 1: {}\n  \
                                     file 2: {}",
                                    key,
                                    entry.definition_path().display(),
                                    path.display()
                                )
                            })?;
                        }
                        Vacant(entry) => {
                            entry.insert(value);
                        }
                    };
                }
            }
            // Allow switching types except for tables or arrays.
            (expected @ &mut CV::List(_, _), found)
            | (expected @ &mut CV::Table(_, _), found)
            | (expected, found @ CV::List(_, _))
            | (expected, found @ CV::Table(_, _)) => {
                return Err(internal(format!(
                    "expected {}, but found {}",
                    expected.desc(),
                    found.desc()
                )));
            }
            _ => {}
        }

        Ok(())
    }

    pub fn i64(&self, key: &str) -> CargoResult<(i64, &Path)> {
        match *self {
            CV::Integer(i, ref p) => Ok((i, p)),
            _ => self.expected("integer", key),
        }
    }

    pub fn string(&self, key: &str) -> CargoResult<(&str, &Path)> {
        match *self {
            CV::String(ref s, ref p) => Ok((s, p)),
            _ => self.expected("string", key),
        }
    }

    pub fn table(&self, key: &str) -> CargoResult<(&HashMap<String, ConfigValue>, &Path)> {
        match *self {
            CV::Table(ref table, ref p) => Ok((table, p)),
            _ => self.expected("table", key),
        }
    }

    pub fn list(&self, key: &str) -> CargoResult<&[(String, PathBuf)]> {
        match *self {
            CV::List(ref list, _) => Ok(list),
            _ => self.expected("list", key),
        }
    }

    pub fn boolean(&self, key: &str) -> CargoResult<(bool, &Path)> {
        match *self {
            CV::Boolean(b, ref p) => Ok((b, p)),
            _ => self.expected("bool", key),
        }
    }

    pub fn desc(&self) -> &'static str {
        match *self {
            CV::Table(..) => "table",
            CV::List(..) => "array",
            CV::String(..) => "string",
            CV::Boolean(..) => "boolean",
            CV::Integer(..) => "integer",
        }
    }

    pub fn definition_path(&self) -> &Path {
        match *self {
            CV::Boolean(_, ref p)
            | CV::Integer(_, ref p)
            | CV::String(_, ref p)
            | CV::List(_, ref p)
            | CV::Table(_, ref p) => p,
        }
    }

    fn expected<T>(&self, wanted: &str, key: &str) -> CargoResult<T> {
        failure::bail!(
            "expected a {}, but found a {} for `{}` in {}",
            wanted,
            self.desc(),
            key,
            self.definition_path().display()
        )
    }
}

pub fn homedir(cwd: &Path) -> Option<PathBuf> {
    ::home::cargo_home_with_cwd(cwd).ok()
}

pub fn save_credentials(cfg: &Config, token: String, registry: Option<String>) -> CargoResult<()> {
    // If 'credentials.toml' exists, we should write to that, otherwise
    // use the legacy 'credentials'. There's no need to print the warning
    // here, because it would already be printed at load time.
    let home_path = cfg.home_path.clone().into_path_unlocked();
    let filename = match cfg.get_file_path(&home_path, "credentials", false)? {
        Some(path) => match path.file_name() {
            Some(filename) => Path::new(filename).to_owned(),
            None => Path::new("credentials").to_owned(),
        },
        None => Path::new("credentials").to_owned(),
    };

    let mut file = {
        cfg.home_path.create_dir()?;
        cfg.home_path
            .open_rw(filename, cfg, "credentials' config file")?
    };

    let (key, value) = {
        let key = "token".to_string();
        let value = ConfigValue::String(token, file.path().to_path_buf());
        let mut map = HashMap::new();
        map.insert(key, value);
        let table = CV::Table(map, file.path().to_path_buf());

        if let Some(registry) = registry {
            let mut map = HashMap::new();
            map.insert(registry, table);
            (
                "registries".into(),
                CV::Table(map, file.path().to_path_buf()),
            )
        } else {
            ("registry".into(), table)
        }
    };

    let mut contents = String::new();
    file.read_to_string(&mut contents).chain_err(|| {
        format!(
            "failed to read configuration file `{}`",
            file.path().display()
        )
    })?;

    let mut toml = cargo_toml::parse(&contents, file.path(), cfg)?;

    // Move the old token location to the new one.
    if let Some(token) = toml.as_table_mut().unwrap().remove("token") {
        let mut map = HashMap::new();
        map.insert("token".to_string(), token);
        toml.as_table_mut()
            .unwrap()
            .insert("registry".into(), map.into());
    }

    toml.as_table_mut().unwrap().insert(key, value.into_toml());

    let contents = toml.to_string();
    file.seek(SeekFrom::Start(0))?;
    file.write_all(contents.as_bytes())?;
    file.file().set_len(contents.len() as u64)?;
    set_permissions(file.file(), 0o600)?;

    return Ok(());

    #[cfg(unix)]
    fn set_permissions(file: &File, mode: u32) -> CargoResult<()> {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = file.metadata()?.permissions();
        perms.set_mode(mode);
        file.set_permissions(perms)?;
        Ok(())
    }

    #[cfg(not(unix))]
    #[allow(unused)]
    fn set_permissions(file: &File, mode: u32) -> CargoResult<()> {
        Ok(())
    }
}

pub struct PackageCacheLock<'a>(&'a Config);

impl Drop for PackageCacheLock<'_> {
    fn drop(&mut self) {
        let mut slot = self.0.package_cache_lock.borrow_mut();
        let (_, cnt) = slot.as_mut().unwrap();
        *cnt -= 1;
        if *cnt == 0 {
            *slot = None;
        }
    }
}

/// returns path to clippy-driver binary
///
/// Allows override of the path via `CARGO_CLIPPY_DRIVER` env variable
pub fn clippy_driver() -> PathBuf {
    env::var("CARGO_CLIPPY_DRIVER")
        .unwrap_or_else(|_| "clippy-driver".into())
        .into()
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct CargoHttpConfig {
    pub proxy: Option<String>,
    pub low_speed_limit: Option<u32>,
    pub timeout: Option<u64>,
    pub cainfo: Option<ConfigRelativePath>,
    pub check_revoke: Option<bool>,
    pub user_agent: Option<String>,
    pub debug: Option<bool>,
    pub multiplexing: Option<bool>,
    pub ssl_version: Option<SslVersionConfig>,
}

/// Configuration for `ssl-version` in `http` section
/// There are two ways to configure:
///
/// ```text
/// [http]
/// ssl-version = "tlsv1.3"
/// ```
///
/// ```text
/// [http]
/// ssl-version.min = "tlsv1.2"
/// ssl-version.max = "tlsv1.3"
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SslVersionConfig {
    Single(String),
    Range(SslVersionConfigRange),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SslVersionConfigRange {
    pub min: Option<String>,
    pub max: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CargoNetConfig {
    pub retry: Option<u32>,
    pub offline: Option<bool>,
    pub git_fetch_with_cli: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CargoBuildConfig {
    pub pipelining: Option<bool>,
    pub dep_info_basedir: Option<ConfigRelativePath>,
    pub target_dir: Option<ConfigRelativePath>,
    pub incremental: Option<bool>,
    pub target: Option<ConfigRelativePath>,
    pub jobs: Option<u32>,
    pub rustflags: Option<StringList>,
    pub rustdocflags: Option<StringList>,
    pub rustc_wrapper: Option<PathBuf>,
    pub rustc: Option<PathBuf>,
    pub rustdoc: Option<PathBuf>,
}

/// A type to deserialize a list of strings from a toml file.
///
/// Supports deserializing either a whitespace-separated list of arguments in a
/// single string or a string list itself. For example these deserialize to
/// equivalent values:
///
/// ```toml
/// a = 'a b c'
/// b = ['a', 'b', 'c']
/// ```
#[derive(Debug)]
pub struct StringList {
    list: Vec<String>,
}

impl StringList {
    pub fn as_slice(&self) -> &[String] {
        &self.list
    }
}

impl<'de> serde::de::Deserialize<'de> for StringList {
    fn deserialize<D: serde::de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Target {
            String(String),
            List(Vec<String>),
        }

        Ok(match Target::deserialize(d)? {
            Target::String(s) => StringList {
                list: s.split_whitespace().map(str::to_string).collect(),
            },
            Target::List(list) => StringList { list },
        })
    }
}
