// ignore-tidy-filelength

//! Rustdoc's HTML rendering module.
//!
//! This modules contains the bulk of the logic necessary for rendering a
//! rustdoc `clean::Crate` instance to a set of static HTML pages. This
//! rendering process is largely driven by the `format!` syntax extension to
//! perform all I/O into files and streams.
//!
//! The rendering process is largely driven by the `Context` and `Cache`
//! structures. The cache is pre-populated by crawling the crate in question,
//! and then it is shared among the various rendering threads. The cache is meant
//! to be a fairly large structure not implementing `Clone` (because it's shared
//! among threads). The context, however, should be a lightweight structure. This
//! is cloned per-thread and contains information about what is currently being
//! rendered.
//!
//! In order to speed up rendering (mostly because of markdown rendering), the
//! rendering process has been parallelized. This parallelization is only
//! exposed through the `crate` method on the context, and then also from the
//! fact that the shared cache is stored in TLS (and must be accessed as such).
//!
//! In addition to rendering the crate itself, this module is also responsible
//! for creating the corresponding search index and source file renderings.
//! These threads are not parallelized (they haven't been a bottleneck yet), and
//! both occur before the crate is rendered.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::default::Default;
use std::error;
use std::fmt::{self, Formatter, Write as FmtWrite};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::path::{PathBuf, Path, Component};
use std::str;
use std::sync::Arc;
use std::rc::Rc;

use errors;
use serialize::json::{ToJson, Json, as_json};
use syntax::ast;
use syntax::edition::Edition;
use syntax::feature_gate::UnstableFeatures;
use syntax::print::pprust;
use syntax::source_map::FileName;
use syntax::symbol::{Symbol, sym};
use syntax_pos::hygiene::MacroKind;
use rustc::hir::def_id::DefId;
use rustc::middle::privacy::AccessLevels;
use rustc::middle::stability;
use rustc::hir;
use rustc::util::nodemap::{FxHashMap, FxHashSet};
use rustc_data_structures::flock;

use crate::clean::{self, AttributesExt, Deprecation, GetDefId, SelfTy, Mutability};
use crate::config::RenderOptions;
use crate::docfs::{DocFS, ErrorStorage, PathError};
use crate::doctree;
use crate::html::escape::Escape;
use crate::html::format::{Buffer, PrintWithSpace, print_abi_with_space};
use crate::html::format::{print_generic_bounds, WhereClause, href, print_default_space};
use crate::html::format::{Function};
use crate::html::format::fmt_impl_for_trait_page;
use crate::html::item_type::ItemType;
use crate::html::markdown::{self, Markdown, MarkdownHtml, MarkdownSummaryLine, ErrorCodes, IdMap};
use crate::html::{highlight, layout, static_files};
use crate::html::sources;

use minifier;

#[cfg(test)]
mod tests;

mod cache;

use cache::Cache;
crate use cache::ExternalLocation::{self, *};

/// A pair of name and its optional document.
pub type NameDoc = (String, Option<String>);

crate fn ensure_trailing_slash(v: &str) -> impl fmt::Display + '_ {
    crate::html::format::display_fn(move |f| {
        if !v.ends_with("/") && !v.is_empty() {
            write!(f, "{}/", v)
        } else {
            write!(f, "{}", v)
        }
    })
}

#[derive(Debug)]
pub struct Error {
    pub file: PathBuf,
    pub error: io::Error,
}

impl error::Error for Error {
    fn description(&self) -> &str {
        self.error.description()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let file = self.file.display().to_string();
        if file.is_empty() {
            write!(f, "{}", self.error)
        } else {
            write!(f, "\"{}\": {}", self.file.display(), self.error)
        }
    }
}

impl PathError for Error {
    fn new<P: AsRef<Path>>(e: io::Error, path: P) -> Error {
        Error {
            file: path.as_ref().to_path_buf(),
            error: e,
        }
    }
}

macro_rules! try_none {
    ($e:expr, $file:expr) => ({
        use std::io;
        match $e {
            Some(e) => e,
            None => return Err(Error::new(io::Error::new(io::ErrorKind::Other, "not found"),
                                          $file))
        }
    })
}

macro_rules! try_err {
    ($e:expr, $file:expr) => ({
        match $e {
            Ok(e) => e,
            Err(e) => return Err(Error::new(e, $file)),
        }
    })
}

/// Major driving force in all rustdoc rendering. This contains information
/// about where in the tree-like hierarchy rendering is occurring and controls
/// how the current page is being rendered.
///
/// It is intended that this context is a lightweight object which can be fairly
/// easily cloned because it is cloned per work-job (about once per item in the
/// rustdoc tree).
#[derive(Clone)]
struct Context {
    /// Current hierarchy of components leading down to what's currently being
    /// rendered
    pub current: Vec<String>,
    /// The current destination folder of where HTML artifacts should be placed.
    /// This changes as the context descends into the module hierarchy.
    pub dst: PathBuf,
    /// A flag, which when `true`, will render pages which redirect to the
    /// real location of an item. This is used to allow external links to
    /// publicly reused items to redirect to the right location.
    pub render_redirect_pages: bool,
    /// The map used to ensure all generated 'id=' attributes are unique.
    id_map: Rc<RefCell<IdMap>>,
    pub shared: Arc<SharedContext>,
    pub cache: Arc<Cache>,
}

crate struct SharedContext {
    /// The path to the crate root source minus the file name.
    /// Used for simplifying paths to the highlighted source code files.
    pub src_root: PathBuf,
    /// This describes the layout of each page, and is not modified after
    /// creation of the context (contains info like the favicon and added html).
    pub layout: layout::Layout,
    /// This flag indicates whether `[src]` links should be generated or not. If
    /// the source files are present in the html rendering, then this will be
    /// `true`.
    pub include_sources: bool,
    /// The local file sources we've emitted and their respective url-paths.
    pub local_sources: FxHashMap<PathBuf, String>,
    /// Whether the collapsed pass ran
    pub collapsed: bool,
    /// The base-URL of the issue tracker for when an item has been tagged with
    /// an issue number.
    pub issue_tracker_base_url: Option<String>,
    /// The directories that have already been created in this doc run. Used to reduce the number
    /// of spurious `create_dir_all` calls.
    pub created_dirs: RefCell<FxHashSet<PathBuf>>,
    /// This flag indicates whether listings of modules (in the side bar and documentation itself)
    /// should be ordered alphabetically or in order of appearance (in the source code).
    pub sort_modules_alphabetically: bool,
    /// Additional themes to be added to the generated docs.
    pub themes: Vec<PathBuf>,
    /// Suffix to be added on resource files (if suffix is "-v2" then "light.css" becomes
    /// "light-v2.css").
    pub resource_suffix: String,
    /// Optional path string to be used to load static files on output pages. If not set, uses
    /// combinations of `../` to reach the documentation root.
    pub static_root_path: Option<String>,
    /// Option disabled by default to generate files used by RLS and some other tools.
    pub generate_redirect_pages: bool,
    /// The fs handle we are working with.
    pub fs: DocFS,
    /// The default edition used to parse doctests.
    pub edition: Edition,
    pub codes: ErrorCodes,
    playground: Option<markdown::Playground>,
}

impl Context {
    fn path(&self, filename: &str) -> PathBuf {
        // We use splitn vs Path::extension here because we might get a filename
        // like `style.min.css` and we want to process that into
        // `style-suffix.min.css`.  Path::extension would just return `css`
        // which would result in `style.min-suffix.css` which isn't what we
        // want.
        let mut iter = filename.splitn(2, '.');
        let base = iter.next().unwrap();
        let ext = iter.next().unwrap();
        let filename = format!(
            "{}{}.{}",
            base,
            self.shared.resource_suffix,
            ext,
        );
        self.dst.join(&filename)
    }
}

impl SharedContext {
    crate fn ensure_dir(&self, dst: &Path) -> Result<(), Error> {
        let mut dirs = self.created_dirs.borrow_mut();
        if !dirs.contains(dst) {
            try_err!(self.fs.create_dir_all(dst), dst);
            dirs.insert(dst.to_path_buf());
        }

        Ok(())
    }

    /// Based on whether the `collapse-docs` pass was run, return either the `doc_value` or the
    /// `collapsed_doc_value` of the given item.
    pub fn maybe_collapsed_doc_value<'a>(&self, item: &'a clean::Item) -> Option<Cow<'a, str>> {
        if self.collapsed {
            item.collapsed_doc_value().map(|s| s.into())
        } else {
            item.doc_value().map(|s| s.into())
        }
    }
}

/// Metadata about implementations for a type or trait.
#[derive(Clone, Debug)]
pub struct Impl {
    pub impl_item: clean::Item,
}

impl Impl {
    fn inner_impl(&self) -> &clean::Impl {
        match self.impl_item.inner {
            clean::ImplItem(ref impl_) => impl_,
            _ => panic!("non-impl item found in impl")
        }
    }

    fn trait_did(&self) -> Option<DefId> {
        self.inner_impl().trait_.def_id()
    }
}

/// Temporary storage for data obtained during `RustdocVisitor::clean()`.
/// Later on moved into `CACHE_KEY`.
#[derive(Default)]
pub struct RenderInfo {
    pub inlined: FxHashSet<DefId>,
    pub external_paths: crate::core::ExternalPaths,
    pub exact_paths: FxHashMap<DefId, Vec<String>>,
    pub access_levels: AccessLevels<DefId>,
    pub deref_trait_did: Option<DefId>,
    pub deref_mut_trait_did: Option<DefId>,
    pub owned_box_did: Option<DefId>,
}

// Helper structs for rendering items/sidebars and carrying along contextual
// information

/// Struct representing one entry in the JS search index. These are all emitted
/// by hand to a large JS file at the end of cache-creation.
#[derive(Debug)]
struct IndexItem {
    ty: ItemType,
    name: String,
    path: String,
    desc: String,
    parent: Option<DefId>,
    parent_idx: Option<usize>,
    search_type: Option<IndexItemFunctionType>,
}

impl ToJson for IndexItem {
    fn to_json(&self) -> Json {
        assert_eq!(self.parent.is_some(), self.parent_idx.is_some());

        let mut data = Vec::with_capacity(6);
        data.push((self.ty as usize).to_json());
        data.push(self.name.to_json());
        data.push(self.path.to_json());
        data.push(self.desc.to_json());
        data.push(self.parent_idx.to_json());
        data.push(self.search_type.to_json());

        Json::Array(data)
    }
}

/// A type used for the search index.
#[derive(Debug)]
struct Type {
    name: Option<String>,
    generics: Option<Vec<String>>,
}

impl ToJson for Type {
    fn to_json(&self) -> Json {
        match self.name {
            Some(ref name) => {
                let mut data = Vec::with_capacity(2);
                data.push(name.to_json());
                if let Some(ref generics) = self.generics {
                    data.push(generics.to_json());
                }
                Json::Array(data)
            }
            None => Json::Null,
        }
    }
}

/// Full type of functions/methods in the search index.
#[derive(Debug)]
struct IndexItemFunctionType {
    inputs: Vec<Type>,
    output: Option<Vec<Type>>,
}

impl ToJson for IndexItemFunctionType {
    fn to_json(&self) -> Json {
        // If we couldn't figure out a type, just write `null`.
        let mut iter = self.inputs.iter();
        if match self.output {
            Some(ref output) => iter.chain(output.iter()).any(|ref i| i.name.is_none()),
            None => iter.any(|ref i| i.name.is_none()),
        } {
            Json::Null
        } else {
            let mut data = Vec::with_capacity(2);
            data.push(self.inputs.to_json());
            if let Some(ref output) = self.output {
                if output.len() > 1 {
                    data.push(output.to_json());
                } else {
                    data.push(output[0].to_json());
                }
            }
            Json::Array(data)
        }
    }
}

thread_local!(static CACHE_KEY: RefCell<Arc<Cache>> = Default::default());
thread_local!(pub static CURRENT_DEPTH: Cell<usize> = Cell::new(0));

pub fn initial_ids() -> Vec<String> {
    [
     "main",
     "search",
     "help",
     "TOC",
     "render-detail",
     "associated-types",
     "associated-const",
     "required-methods",
     "provided-methods",
     "implementors",
     "synthetic-implementors",
     "implementors-list",
     "synthetic-implementors-list",
     "methods",
     "deref-methods",
     "implementations",
    ].iter().map(|id| (String::from(*id))).collect()
}

/// Generates the documentation for `crate` into the directory `dst`
pub fn run(mut krate: clean::Crate,
           options: RenderOptions,
           renderinfo: RenderInfo,
           diag: &errors::Handler,
           edition: Edition) -> Result<(), Error> {
    // need to save a copy of the options for rendering the index page
    let md_opts = options.clone();
    let RenderOptions {
        output,
        external_html,
        id_map,
        playground_url,
        sort_modules_alphabetically,
        themes,
        extension_css,
        extern_html_root_urls,
        resource_suffix,
        static_root_path,
        generate_search_filter,
        generate_redirect_pages,
        ..
    } = options;

    let src_root = match krate.src {
        FileName::Real(ref p) => match p.parent() {
            Some(p) => p.to_path_buf(),
            None => PathBuf::new(),
        },
        _ => PathBuf::new(),
    };
    let mut errors = Arc::new(ErrorStorage::new());
    // If user passed in `--playground-url` arg, we fill in crate name here
    let mut playground = None;
    if let Some(url) = playground_url {
        playground = Some(markdown::Playground {
            crate_name: Some(krate.name.clone()),
            url,
        });
    }
    let mut layout = layout::Layout {
        logo: String::new(),
        favicon: String::new(),
        external_html,
        krate: krate.name.clone(),
        css_file_extension: extension_css,
        generate_search_filter,
    };
    let mut issue_tracker_base_url = None;
    let mut include_sources = true;

    // Crawl the crate attributes looking for attributes which control how we're
    // going to emit HTML
    if let Some(attrs) = krate.module.as_ref().map(|m| &m.attrs) {
        for attr in attrs.lists(sym::doc) {
            match (attr.name_or_empty(), attr.value_str()) {
                (sym::html_favicon_url, Some(s)) => {
                    layout.favicon = s.to_string();
                }
                (sym::html_logo_url, Some(s)) => {
                    layout.logo = s.to_string();
                }
                (sym::html_playground_url, Some(s)) => {
                    playground = Some(markdown::Playground {
                        crate_name: Some(krate.name.clone()),
                        url: s.to_string(),
                    });
                }
                (sym::issue_tracker_base_url, Some(s)) => {
                    issue_tracker_base_url = Some(s.to_string());
                }
                (sym::html_no_source, None) if attr.is_word() => {
                    include_sources = false;
                }
                _ => {}
            }
        }
    }
    let mut scx = SharedContext {
        collapsed: krate.collapsed,
        src_root,
        include_sources,
        local_sources: Default::default(),
        issue_tracker_base_url,
        layout,
        created_dirs: Default::default(),
        sort_modules_alphabetically,
        themes,
        resource_suffix,
        static_root_path,
        generate_redirect_pages,
        fs: DocFS::new(&errors),
        edition,
        codes: ErrorCodes::from(UnstableFeatures::from_environment().is_nightly_build()),
        playground,
    };

    let dst = output;
    scx.ensure_dir(&dst)?;
    krate = sources::render(&dst, &mut scx, krate)?;
    let (new_crate, index, cache) = Cache::from_krate(
        renderinfo,
        &extern_html_root_urls,
        &dst,
        krate,
    );
    krate = new_crate;
    let cache = Arc::new(cache);
    let mut cx = Context {
        current: Vec::new(),
        dst,
        render_redirect_pages: false,
        id_map: Rc::new(RefCell::new(id_map)),
        shared: Arc::new(scx),
        cache: cache.clone(),
    };

    // Freeze the cache now that the index has been built. Put an Arc into TLS
    // for future parallelization opportunities
    CACHE_KEY.with(|v| *v.borrow_mut() = cache.clone());
    CURRENT_DEPTH.with(|s| s.set(0));

    // Write shared runs within a flock; disable thread dispatching of IO temporarily.
    Arc::get_mut(&mut cx.shared).unwrap().fs.set_sync_only(true);
    write_shared(&cx, &krate, index, &md_opts, diag)?;
    Arc::get_mut(&mut cx.shared).unwrap().fs.set_sync_only(false);

    // And finally render the whole crate's documentation
    let ret = cx.krate(krate);
    let nb_errors = Arc::get_mut(&mut errors).map_or_else(|| 0, |errors| errors.write_errors(diag));
    if ret.is_err() {
        ret
    } else if nb_errors > 0 {
        Err(Error::new(io::Error::new(io::ErrorKind::Other, "I/O error"), ""))
    } else {
        Ok(())
    }
}

fn write_shared(
    cx: &Context,
    krate: &clean::Crate,
    search_index: String,
    options: &RenderOptions,
    diag: &errors::Handler,
) -> Result<(), Error> {
    // Write out the shared files. Note that these are shared among all rustdoc
    // docs placed in the output directory, so this needs to be a synchronized
    // operation with respect to all other rustdocs running around.
    let lock_file = cx.dst.join(".lock");
    let _lock = try_err!(flock::Lock::new(&lock_file, true, true, true), &lock_file);

    // Add all the static files. These may already exist, but we just
    // overwrite them anyway to make sure that they're fresh and up-to-date.

    write_minify(&cx.shared.fs, cx.path("rustdoc.css"),
                 static_files::RUSTDOC_CSS,
                 options.enable_minification)?;
    write_minify(&cx.shared.fs, cx.path("settings.css"),
                 static_files::SETTINGS_CSS,
                 options.enable_minification)?;
    write_minify(&cx.shared.fs, cx.path("noscript.css"),
                 static_files::NOSCRIPT_CSS,
                 options.enable_minification)?;

    // To avoid "light.css" to be overwritten, we'll first run over the received themes and only
    // then we'll run over the "official" styles.
    let mut themes: FxHashSet<String> = FxHashSet::default();

    for entry in &cx.shared.themes {
        let content = try_err!(fs::read(&entry), &entry);
        let theme = try_none!(try_none!(entry.file_stem(), &entry).to_str(), &entry);
        let extension = try_none!(try_none!(entry.extension(), &entry).to_str(), &entry);
        cx.shared.fs.write(cx.path(&format!("{}.{}", theme, extension)), content.as_slice())?;
        themes.insert(theme.to_owned());
    }

    let write = |p, c| { cx.shared.fs.write(p, c) };
    if (*cx.shared).layout.logo.is_empty() {
        write(cx.path("rust-logo.png"), static_files::RUST_LOGO)?;
    }
    if (*cx.shared).layout.favicon.is_empty() {
        write(cx.path("favicon.ico"), static_files::RUST_FAVICON)?;
    }
    write(cx.path("brush.svg"), static_files::BRUSH_SVG)?;
    write(cx.path("wheel.svg"), static_files::WHEEL_SVG)?;
    write(cx.path("down-arrow.svg"), static_files::DOWN_ARROW_SVG)?;
    write_minify(&cx.shared.fs,
        cx.path("light.css"), static_files::themes::LIGHT, options.enable_minification)?;
    themes.insert("light".to_owned());
    write_minify(&cx.shared.fs,
        cx.path("dark.css"), static_files::themes::DARK, options.enable_minification)?;
    themes.insert("dark".to_owned());

    let mut themes: Vec<&String> = themes.iter().collect();
    themes.sort();
    // To avoid theme switch latencies as much as possible, we put everything theme related
    // at the beginning of the html files into another js file.
    let theme_js = format!(
r#"var themes = document.getElementById("theme-choices");
var themePicker = document.getElementById("theme-picker");

function showThemeButtonState() {{
    themes.style.display = "block";
    themePicker.style.borderBottomRightRadius = "0";
    themePicker.style.borderBottomLeftRadius = "0";
}}

function hideThemeButtonState() {{
    themes.style.display = "none";
    themePicker.style.borderBottomRightRadius = "3px";
    themePicker.style.borderBottomLeftRadius = "3px";
}}

function switchThemeButtonState() {{
    if (themes.style.display === "block") {{
        hideThemeButtonState();
    }} else {{
        showThemeButtonState();
    }}
}};

function handleThemeButtonsBlur(e) {{
    var active = document.activeElement;
    var related = e.relatedTarget;

    if (active.id !== "themePicker" &&
        (!active.parentNode || active.parentNode.id !== "theme-choices") &&
        (!related ||
         (related.id !== "themePicker" &&
          (!related.parentNode || related.parentNode.id !== "theme-choices")))) {{
        hideThemeButtonState();
    }}
}}

themePicker.onclick = switchThemeButtonState;
themePicker.onblur = handleThemeButtonsBlur;
{}.forEach(function(item) {{
    var but = document.createElement('button');
    but.textContent = item;
    but.onclick = function(el) {{
        switchTheme(currentTheme, mainTheme, item, true);
    }};
    but.onblur = handleThemeButtonsBlur;
    themes.appendChild(but);
}});"#,
                 as_json(&themes));
    write(cx.dst.join(&format!("theme{}.js", cx.shared.resource_suffix)),
          theme_js.as_bytes()
    )?;

    write_minify(&cx.shared.fs, cx.path("main.js"),
                 static_files::MAIN_JS,
                 options.enable_minification)?;
    write_minify(&cx.shared.fs, cx.path("settings.js"),
                 static_files::SETTINGS_JS,
                 options.enable_minification)?;
    if cx.shared.include_sources {
        write_minify(
            &cx.shared.fs,
            cx.path("source-script.js"),
            static_files::sidebar::SOURCE_SCRIPT,
            options.enable_minification)?;
    }

    {
        write_minify(
            &cx.shared.fs,
            cx.path("storage.js"),
            &format!("var resourcesSuffix = \"{}\";{}",
                     cx.shared.resource_suffix,
                     static_files::STORAGE_JS),
            options.enable_minification)?;
    }

    if let Some(ref css) = cx.shared.layout.css_file_extension {
        let out = cx.path("theme.css");
        let buffer = try_err!(fs::read_to_string(css), css);
        if !options.enable_minification {
            cx.shared.fs.write(&out, &buffer)?;
        } else {
            write_minify(&cx.shared.fs, out, &buffer, options.enable_minification)?;
        }
    }
    write_minify(&cx.shared.fs, cx.path("normalize.css"),
                 static_files::NORMALIZE_CSS,
                 options.enable_minification)?;
    write(cx.dst.join("FiraSans-Regular.woff"),
          static_files::fira_sans::REGULAR)?;
    write(cx.dst.join("FiraSans-Medium.woff"),
          static_files::fira_sans::MEDIUM)?;
    write(cx.dst.join("FiraSans-LICENSE.txt"),
          static_files::fira_sans::LICENSE)?;
    write(cx.dst.join("SourceSerifPro-Regular.ttf.woff"),
          static_files::source_serif_pro::REGULAR)?;
    write(cx.dst.join("SourceSerifPro-Bold.ttf.woff"),
          static_files::source_serif_pro::BOLD)?;
    write(cx.dst.join("SourceSerifPro-It.ttf.woff"),
          static_files::source_serif_pro::ITALIC)?;
    write(cx.dst.join("SourceSerifPro-LICENSE.md"),
          static_files::source_serif_pro::LICENSE)?;
    write(cx.dst.join("SourceCodePro-Regular.woff"),
          static_files::source_code_pro::REGULAR)?;
    write(cx.dst.join("SourceCodePro-Semibold.woff"),
          static_files::source_code_pro::SEMIBOLD)?;
    write(cx.dst.join("SourceCodePro-LICENSE.txt"),
          static_files::source_code_pro::LICENSE)?;
    write(cx.dst.join("LICENSE-MIT.txt"),
          static_files::LICENSE_MIT)?;
    write(cx.dst.join("LICENSE-APACHE.txt"),
          static_files::LICENSE_APACHE)?;
    write(cx.dst.join("COPYRIGHT.txt"),
          static_files::COPYRIGHT)?;

    fn collect(
        path: &Path,
        krate: &str,
        key: &str,
        for_search_index: bool,
    ) -> io::Result<(Vec<String>, Vec<String>, Vec<String>)> {
        let mut ret = Vec::new();
        let mut krates = Vec::new();
        let mut variables = Vec::new();

        if path.exists() {
            for line in BufReader::new(File::open(path)?).lines() {
                let line = line?;
                if for_search_index && line.starts_with("var R") {
                    variables.push(line.clone());
                    continue;
                }
                if !line.starts_with(key) {
                    continue;
                }
                if line.starts_with(&format!(r#"{}["{}"]"#, key, krate)) {
                    continue;
                }
                ret.push(line.to_string());
                krates.push(line[key.len() + 2..].split('"')
                                                 .next()
                                                 .map(|s| s.to_owned())
                                                 .unwrap_or_else(|| String::new()));
            }
        }
        Ok((ret, krates, variables))
    }

    fn show_item(item: &IndexItem, krate: &str) -> String {
        format!("{{'crate':'{}','ty':{},'name':'{}','desc':'{}','p':'{}'{}}}",
                krate, item.ty as usize, item.name, item.desc.replace("'", "\\'"), item.path,
                if let Some(p) = item.parent_idx {
                    format!(",'parent':{}", p)
                } else {
                    String::new()
                })
    }

    let dst = cx.dst.join(&format!("aliases{}.js", cx.shared.resource_suffix));
    {
        let (mut all_aliases, _, _) = try_err!(collect(&dst, &krate.name, "ALIASES", false), &dst);
        let mut output = String::with_capacity(100);
        for (alias, items) in &cx.cache.aliases {
            if items.is_empty() {
                continue
            }
            output.push_str(&format!("\"{}\":[{}],",
                                     alias,
                                     items.iter()
                                          .map(|v| show_item(v, &krate.name))
                                          .collect::<Vec<_>>()
                                          .join(",")));
        }
        all_aliases.push(format!("ALIASES[\"{}\"] = {{{}}};", krate.name, output));
        all_aliases.sort();
        let mut v = Buffer::html();
        writeln!(&mut v, "var ALIASES = {{}};");
        for aliases in &all_aliases {
            writeln!(&mut v, "{}", aliases);
        }
        cx.shared.fs.write(&dst, v.into_inner().into_bytes())?;
    }

    use std::ffi::OsString;

    #[derive(Debug)]
    struct Hierarchy {
        elem: OsString,
        children: FxHashMap<OsString, Hierarchy>,
        elems: FxHashSet<OsString>,
    }

    impl Hierarchy {
        fn new(elem: OsString) -> Hierarchy {
            Hierarchy {
                elem,
                children: FxHashMap::default(),
                elems: FxHashSet::default(),
            }
        }

        fn to_json_string(&self) -> String {
            let mut subs: Vec<&Hierarchy> = self.children.values().collect();
            subs.sort_unstable_by(|a, b| a.elem.cmp(&b.elem));
            let mut files = self.elems.iter()
                                      .map(|s| format!("\"{}\"",
                                                       s.to_str()
                                                        .expect("invalid osstring conversion")))
                                      .collect::<Vec<_>>();
            files.sort_unstable_by(|a, b| a.cmp(b));
            let subs = subs.iter().map(|s| s.to_json_string()).collect::<Vec<_>>().join(",");
            let dirs = if subs.is_empty() {
                String::new()
            } else {
                format!(",\"dirs\":[{}]", subs)
            };
            let files = files.join(",");
            let files = if files.is_empty() {
                String::new()
            } else {
                format!(",\"files\":[{}]", files)
            };
            format!("{{\"name\":\"{name}\"{dirs}{files}}}",
                    name=self.elem.to_str().expect("invalid osstring conversion"),
                    dirs=dirs,
                    files=files)
        }
    }

    if cx.shared.include_sources {
        let mut hierarchy = Hierarchy::new(OsString::new());
        for source in cx.shared.local_sources.iter()
                                             .filter_map(|p| p.0.strip_prefix(&cx.shared.src_root)
                                                                .ok()) {
            let mut h = &mut hierarchy;
            let mut elems = source.components()
                                  .filter_map(|s| {
                                      match s {
                                          Component::Normal(s) => Some(s.to_owned()),
                                          _ => None,
                                      }
                                  })
                                  .peekable();
            loop {
                let cur_elem = elems.next().expect("empty file path");
                if elems.peek().is_none() {
                    h.elems.insert(cur_elem);
                    break;
                } else {
                    let e = cur_elem.clone();
                    h.children.entry(cur_elem.clone()).or_insert_with(|| Hierarchy::new(e));
                    h = h.children.get_mut(&cur_elem).expect("not found child");
                }
            }
        }

        let dst = cx.dst.join(&format!("source-files{}.js", cx.shared.resource_suffix));
        let (mut all_sources, _krates, _) = try_err!(collect(&dst, &krate.name, "sourcesIndex",
                                                             false),
                                                     &dst);
        all_sources.push(format!("sourcesIndex[\"{}\"] = {};",
                                 &krate.name,
                                 hierarchy.to_json_string()));
        all_sources.sort();
        let v = format!("var N = null;var sourcesIndex = {{}};\n{}\ncreateSourceSidebar();\n",
                          all_sources.join("\n"));
        cx.shared.fs.write(&dst, v.as_bytes())?;
    }

    // Update the search index
    let dst = cx.dst.join(&format!("search-index{}.js", cx.shared.resource_suffix));
    let (mut all_indexes, mut krates, variables) = try_err!(collect(&dst,
                                                                    &krate.name,
                                                                    "searchIndex",
                                                                    true), &dst);
    all_indexes.push(search_index);

    // Sort the indexes by crate so the file will be generated identically even
    // with rustdoc running in parallel.
    all_indexes.sort();
    {
        let mut v = String::from("var N=null,E=\"\",T=\"t\",U=\"u\",searchIndex={};\n");
        v.push_str(&minify_replacer(
            &format!("{}\n{}", variables.join(""), all_indexes.join("\n")),
            options.enable_minification));
        // "addSearchOptions" has to be called first so the crate filtering can be set before the
        // search might start (if it's set into the URL for example).
        v.push_str("addSearchOptions(searchIndex);initSearch(searchIndex);");
        cx.shared.fs.write(&dst, &v)?;
    }
    if options.enable_index_page {
        if let Some(index_page) = options.index_page.clone() {
            let mut md_opts = options.clone();
            md_opts.output = cx.dst.clone();
            md_opts.external_html = (*cx.shared).layout.external_html.clone();

            crate::markdown::render(index_page, md_opts, diag, cx.shared.edition);
        } else {
            let dst = cx.dst.join("index.html");
            let page = layout::Page {
                title: "Index of crates",
                css_class: "mod",
                root_path: "./",
                static_root_path: cx.shared.static_root_path.as_deref(),
                description: "List of crates",
                keywords: BASIC_KEYWORDS,
                resource_suffix: &cx.shared.resource_suffix,
                extra_scripts: &[],
                static_extra_scripts: &[],
            };
            krates.push(krate.name.clone());
            krates.sort();
            krates.dedup();

            let content = format!(
"<h1 class='fqn'>\
     <span class='in-band'>List of all crates</span>\
</h1><ul class='mod'>{}</ul>",
                                  krates
                                    .iter()
                                    .map(|s| {
                                        format!("<li><a href=\"{}index.html\">{}</li>",
                                                ensure_trailing_slash(s), s)
                                    })
                                    .collect::<String>());
            let v = layout::render(&cx.shared.layout,
                           &page, "", content,
                           &cx.shared.themes);
            cx.shared.fs.write(&dst, v.as_bytes())?;
        }
    }

    // Update the list of all implementors for traits
    let dst = cx.dst.join("implementors");
    for (&did, imps) in &cx.cache.implementors {
        // Private modules can leak through to this phase of rustdoc, which
        // could contain implementations for otherwise private types. In some
        // rare cases we could find an implementation for an item which wasn't
        // indexed, so we just skip this step in that case.
        //
        // FIXME: this is a vague explanation for why this can't be a `get`, in
        //        theory it should be...
        let &(ref remote_path, remote_item_type) = match cx.cache.paths.get(&did) {
            Some(p) => p,
            None => match cx.cache.external_paths.get(&did) {
                Some(p) => p,
                None => continue,
            }
        };

        let mut have_impls = false;
        let mut implementors = format!(r#"implementors["{}"] = ["#, krate.name);
        for imp in imps {
            // If the trait and implementation are in the same crate, then
            // there's no need to emit information about it (there's inlining
            // going on). If they're in different crates then the crate defining
            // the trait will be interested in our implementation.
            if imp.impl_item.def_id.krate == did.krate { continue }
            // If the implementation is from another crate then that crate
            // should add it.
            if !imp.impl_item.def_id.is_local() { continue }
            have_impls = true;
            write!(implementors, "{{text:{},synthetic:{},types:{}}},",
                   as_json(&imp.inner_impl().print().to_string()),
                   imp.inner_impl().synthetic,
                   as_json(&collect_paths_for_type(imp.inner_impl().for_.clone()))).unwrap();
        }
        implementors.push_str("];");

        // Only create a js file if we have impls to add to it. If the trait is
        // documented locally though we always create the file to avoid dead
        // links.
        if !have_impls && !cx.cache.paths.contains_key(&did) {
            continue;
        }

        let mut mydst = dst.clone();
        for part in &remote_path[..remote_path.len() - 1] {
            mydst.push(part);
        }
        cx.shared.ensure_dir(&mydst)?;
        mydst.push(&format!("{}.{}.js",
                            remote_item_type,
                            remote_path[remote_path.len() - 1]));

        let (mut all_implementors, _, _) = try_err!(collect(&mydst, &krate.name, "implementors",
                                                            false),
                                                    &mydst);
        all_implementors.push(implementors);
        // Sort the implementors by crate so the file will be generated
        // identically even with rustdoc running in parallel.
        all_implementors.sort();

        let mut v = String::from("(function() {var implementors = {};\n");
        for implementor in &all_implementors {
            writeln!(v, "{}", *implementor).unwrap();
        }
        v.push_str(r"
            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        ");
        v.push_str("})()");
        cx.shared.fs.write(&mydst, &v)?;
    }
    Ok(())
}

fn write_minify(fs:&DocFS, dst: PathBuf, contents: &str, enable_minification: bool
                ) -> Result<(), Error> {
    if enable_minification {
        if dst.extension() == Some(&OsStr::new("css")) {
            let res = try_none!(minifier::css::minify(contents).ok(), &dst);
            fs.write(dst, res.as_bytes())
        } else {
            fs.write(dst, minifier::js::minify(contents).as_bytes())
        }
    } else {
        fs.write(dst, contents.as_bytes())
    }
}

fn minify_replacer(
    contents: &str,
    enable_minification: bool,
) -> String {
    use minifier::js::{simple_minify, Keyword, ReservedChar, Token, Tokens};

    if enable_minification {
        let tokens: Tokens<'_> = simple_minify(contents)
            .into_iter()
            .filter(|(f, next)| {
                // We keep backlines.
                minifier::js::clean_token_except(f, next, &|c: &Token<'_>| {
                    c.get_char() != Some(ReservedChar::Backline)
                })
            })
            .map(|(f, _)| {
                minifier::js::replace_token_with(f, &|t: &Token<'_>| {
                    match *t {
                        Token::Keyword(Keyword::Null) => Some(Token::Other("N")),
                        Token::String(s) => {
                            let s = &s[1..s.len() -1]; // The quotes are included
                            if s.is_empty() {
                                Some(Token::Other("E"))
                            } else if s == "t" {
                                Some(Token::Other("T"))
                            } else if s == "u" {
                                Some(Token::Other("U"))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                })
            })
            .collect::<Vec<_>>()
            .into();
        let o = tokens.apply(|f| {
            // We add a backline after the newly created variables.
            minifier::js::aggregate_strings_into_array_with_separation_filter(
                f,
                "R",
                Token::Char(ReservedChar::Backline),
                // This closure prevents crates' names from being aggregated.
                //
                // The point here is to check if the string is preceded by '[' and
                // "searchIndex". If so, it means this is a crate name and that it
                // shouldn't be aggregated.
                |tokens, pos| {
                    pos < 2 ||
                    !tokens[pos - 1].eq_char(ReservedChar::OpenBracket) ||
                    tokens[pos - 2].get_other() != Some("searchIndex")
                }
            )
        })
        .to_string();
        format!("{}\n", o)
    } else {
        format!("{}\n", contents)
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
struct ItemEntry {
    url: String,
    name: String,
}

impl ItemEntry {
    fn new(mut url: String, name: String) -> ItemEntry {
        while url.starts_with('/') {
            url.remove(0);
        }
        ItemEntry {
            url,
            name,
        }
    }
}

impl ItemEntry {
    crate fn print(&self) -> impl fmt::Display + '_ {
        crate::html::format::display_fn(move |f| {
            write!(f, "<a href='{}'>{}</a>", self.url, Escape(&self.name))
        })
    }
}

impl PartialOrd for ItemEntry {
    fn partial_cmp(&self, other: &ItemEntry) -> Option<::std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ItemEntry {
    fn cmp(&self, other: &ItemEntry) -> ::std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

#[derive(Debug)]
struct AllTypes {
    structs: FxHashSet<ItemEntry>,
    enums: FxHashSet<ItemEntry>,
    unions: FxHashSet<ItemEntry>,
    primitives: FxHashSet<ItemEntry>,
    traits: FxHashSet<ItemEntry>,
    macros: FxHashSet<ItemEntry>,
    functions: FxHashSet<ItemEntry>,
    typedefs: FxHashSet<ItemEntry>,
    opaque_tys: FxHashSet<ItemEntry>,
    statics: FxHashSet<ItemEntry>,
    constants: FxHashSet<ItemEntry>,
    keywords: FxHashSet<ItemEntry>,
    attributes: FxHashSet<ItemEntry>,
    derives: FxHashSet<ItemEntry>,
    trait_aliases: FxHashSet<ItemEntry>,
}

impl AllTypes {
    fn new() -> AllTypes {
        let new_set = |cap| FxHashSet::with_capacity_and_hasher(cap, Default::default());
        AllTypes {
            structs: new_set(100),
            enums: new_set(100),
            unions: new_set(100),
            primitives: new_set(26),
            traits: new_set(100),
            macros: new_set(100),
            functions: new_set(100),
            typedefs: new_set(100),
            opaque_tys: new_set(100),
            statics: new_set(100),
            constants: new_set(100),
            keywords: new_set(100),
            attributes: new_set(100),
            derives: new_set(100),
            trait_aliases: new_set(100),
        }
    }

    fn append(&mut self, item_name: String, item_type: &ItemType) {
        let mut url: Vec<_> = item_name.split("::").skip(1).collect();
        if let Some(name) = url.pop() {
            let new_url = format!("{}/{}.{}.html", url.join("/"), item_type, name);
            url.push(name);
            let name = url.join("::");
            match *item_type {
                ItemType::Struct => self.structs.insert(ItemEntry::new(new_url, name)),
                ItemType::Enum => self.enums.insert(ItemEntry::new(new_url, name)),
                ItemType::Union => self.unions.insert(ItemEntry::new(new_url, name)),
                ItemType::Primitive => self.primitives.insert(ItemEntry::new(new_url, name)),
                ItemType::Trait => self.traits.insert(ItemEntry::new(new_url, name)),
                ItemType::Macro => self.macros.insert(ItemEntry::new(new_url, name)),
                ItemType::Function => self.functions.insert(ItemEntry::new(new_url, name)),
                ItemType::Typedef => self.typedefs.insert(ItemEntry::new(new_url, name)),
                ItemType::OpaqueTy => self.opaque_tys.insert(ItemEntry::new(new_url, name)),
                ItemType::Static => self.statics.insert(ItemEntry::new(new_url, name)),
                ItemType::Constant => self.constants.insert(ItemEntry::new(new_url, name)),
                ItemType::ProcAttribute => self.attributes.insert(ItemEntry::new(new_url, name)),
                ItemType::ProcDerive => self.derives.insert(ItemEntry::new(new_url, name)),
                ItemType::TraitAlias => self.trait_aliases.insert(ItemEntry::new(new_url, name)),
                _ => true,
            };
        }
    }
}

fn print_entries(f: &mut Buffer, e: &FxHashSet<ItemEntry>, title: &str, class: &str) {
    if !e.is_empty() {
        let mut e: Vec<&ItemEntry> = e.iter().collect();
        e.sort();
        write!(f, "<h3 id='{}'>{}</h3><ul class='{} docblock'>{}</ul>",
               title,
               Escape(title),
               class,
               e.iter().map(|s| format!("<li>{}</li>", s.print())).collect::<String>());
    }
}

impl AllTypes {
    fn print(self, f: &mut Buffer) {
        write!(f,
    "<h1 class='fqn'>\
        <span class='out-of-band'>\
            <span id='render-detail'>\
                <a id=\"toggle-all-docs\" href=\"javascript:void(0)\" title=\"collapse all docs\">\
                    [<span class='inner'>&#x2212;</span>]\
                </a>\
            </span>
        </span>
        <span class='in-band'>List of all items</span>\
    </h1>");
        print_entries(f, &self.structs, "Structs", "structs");
        print_entries(f, &self.enums, "Enums", "enums");
        print_entries(f, &self.unions, "Unions", "unions");
        print_entries(f, &self.primitives, "Primitives", "primitives");
        print_entries(f, &self.traits, "Traits", "traits");
        print_entries(f, &self.macros, "Macros", "macros");
        print_entries(f, &self.attributes, "Attribute Macros", "attributes");
        print_entries(f, &self.derives, "Derive Macros", "derives");
        print_entries(f, &self.functions, "Functions", "functions");
        print_entries(f, &self.typedefs, "Typedefs", "typedefs");
        print_entries(f, &self.trait_aliases, "Trait Aliases", "trait-aliases");
        print_entries(f, &self.opaque_tys, "Opaque Types", "opaque-types");
        print_entries(f, &self.statics, "Statics", "statics");
        print_entries(f, &self.constants, "Constants", "constants")
    }
}

#[derive(Debug)]
enum Setting {
    Section {
        description: &'static str,
        sub_settings: Vec<Setting>,
    },
    Entry {
        js_data_name: &'static str,
        description: &'static str,
        default_value: bool,
    }
}

impl Setting {
    fn display(&self) -> String {
        match *self {
            Setting::Section { ref description, ref sub_settings } => {
                format!(
                    "<div class='setting-line'>\
                        <div class='title'>{}</div>\
                        <div class='sub-settings'>{}</div>
                    </div>",
                    description,
                    sub_settings.iter().map(|s| s.display()).collect::<String>()
                )
            }
            Setting::Entry { ref js_data_name, ref description, ref default_value } => {
                format!(
                    "<div class='setting-line'>\
                        <label class='toggle'>\
                        <input type='checkbox' id='{}' {}>\
                        <span class='slider'></span>\
                        </label>\
                        <div>{}</div>\
                    </div>",
                    js_data_name,
                    if *default_value { " checked" } else { "" },
                    description,
                )
            }
        }
    }
}

impl From<(&'static str, &'static str, bool)> for Setting {
    fn from(values: (&'static str, &'static str, bool)) -> Setting {
        Setting::Entry {
            js_data_name: values.0,
            description: values.1,
            default_value: values.2,
        }
    }
}

impl<T: Into<Setting>> From<(&'static str, Vec<T>)> for Setting {
    fn from(values: (&'static str, Vec<T>)) -> Setting {
        Setting::Section {
            description: values.0,
            sub_settings: values.1.into_iter().map(|v| v.into()).collect::<Vec<_>>(),
        }
    }
}

fn settings(root_path: &str, suffix: &str) -> String {
    // (id, explanation, default value)
    let settings: &[Setting] = &[
        ("Auto-hide item declarations", vec![
            ("auto-hide-struct", "Auto-hide structs declaration", true),
            ("auto-hide-enum", "Auto-hide enums declaration", false),
            ("auto-hide-union", "Auto-hide unions declaration", true),
            ("auto-hide-trait", "Auto-hide traits declaration", true),
            ("auto-hide-macro", "Auto-hide macros declaration", false),
        ]).into(),
        ("auto-hide-attributes", "Auto-hide item attributes.", true).into(),
        ("auto-hide-method-docs", "Auto-hide item methods' documentation", false).into(),
        ("auto-hide-trait-implementations", "Auto-hide trait implementations documentation",
            true).into(),
        ("go-to-only-result", "Directly go to item in search if there is only one result",
            false).into(),
        ("line-numbers", "Show line numbers on code examples", false).into(),
        ("disable-shortcuts", "Disable keyboard shortcuts", false).into(),
    ];
    format!(
"<h1 class='fqn'>\
    <span class='in-band'>Rustdoc settings</span>\
</h1>\
<div class='settings'>{}</div>\
<script src='{}settings{}.js'></script>",
            settings.iter().map(|s| s.display()).collect::<String>(),
            root_path,
            suffix)
}

impl Context {
    fn derive_id(&self, id: String) -> String {
        let mut map = self.id_map.borrow_mut();
        map.derive(id)
    }

    /// String representation of how to get back to the root path of the 'doc/'
    /// folder in terms of a relative URL.
    fn root_path(&self) -> String {
        "../".repeat(self.current.len())
    }

    /// Main method for rendering a crate.
    ///
    /// This currently isn't parallelized, but it'd be pretty easy to add
    /// parallelization to this function.
    fn krate(self, mut krate: clean::Crate) -> Result<(), Error> {
        let mut item = match krate.module.take() {
            Some(i) => i,
            None => return Ok(()),
        };
        let final_file = self.dst.join(&krate.name)
                                 .join("all.html");
        let settings_file = self.dst.join("settings.html");

        let crate_name = krate.name.clone();
        item.name = Some(krate.name);

        let mut all = AllTypes::new();

        {
            // Render the crate documentation
            let mut work = vec![(self.clone(), item)];

            while let Some((mut cx, item)) = work.pop() {
                cx.item(item, &mut all, |cx, item| {
                    work.push((cx.clone(), item))
                })?
            }
        }

        let mut root_path = self.dst.to_str().expect("invalid path").to_owned();
        if !root_path.ends_with('/') {
            root_path.push('/');
        }
        let mut page = layout::Page {
            title: "List of all items in this crate",
            css_class: "mod",
            root_path: "../",
            static_root_path: self.shared.static_root_path.as_deref(),
            description: "List of all items in this crate",
            keywords: BASIC_KEYWORDS,
            resource_suffix: &self.shared.resource_suffix,
            extra_scripts: &[],
            static_extra_scripts: &[],
        };
        let sidebar = if let Some(ref version) = self.cache.crate_version {
            format!("<p class='location'>Crate {}</p>\
                     <div class='block version'>\
                         <p>Version {}</p>\
                     </div>\
                     <a id='all-types' href='index.html'><p>Back to index</p></a>",
                    crate_name, version)
        } else {
            String::new()
        };
        let v = layout::render(&self.shared.layout,
                       &page, sidebar, |buf: &mut Buffer| all.print(buf),
                       &self.shared.themes);
        self.shared.fs.write(&final_file, v.as_bytes())?;

        // Generating settings page.
        page.title = "Rustdoc settings";
        page.description = "Settings of Rustdoc";
        page.root_path = "./";

        let mut themes = self.shared.themes.clone();
        let sidebar = "<p class='location'>Settings</p><div class='sidebar-elems'></div>";
        themes.push(PathBuf::from("settings.css"));
        let v = layout::render(
            &self.shared.layout,
            &page, sidebar, settings(
                self.shared.static_root_path.as_deref().unwrap_or("./"),
                &self.shared.resource_suffix
            ),
            &themes);
        self.shared.fs.write(&settings_file, v.as_bytes())?;

        Ok(())
    }

    fn render_item(&self,
                   it: &clean::Item,
                   pushname: bool) -> String {
        // A little unfortunate that this is done like this, but it sure
        // does make formatting *a lot* nicer.
        CURRENT_DEPTH.with(|slot| {
            slot.set(self.current.len());
        });

        let mut title = if it.is_primitive() || it.is_keyword() {
            // No need to include the namespace for primitive types and keywords
            String::new()
        } else {
            self.current.join("::")
        };
        if pushname {
            if !title.is_empty() {
                title.push_str("::");
            }
            title.push_str(it.name.as_ref().unwrap());
        }
        title.push_str(" - Rust");
        let tyname = it.type_();
        let desc = if it.is_crate() {
            format!("API documentation for the Rust `{}` crate.",
                    self.shared.layout.krate)
        } else {
            format!("API documentation for the Rust `{}` {} in crate `{}`.",
                    it.name.as_ref().unwrap(), tyname, self.shared.layout.krate)
        };
        let keywords = make_item_keywords(it);
        let page = layout::Page {
            css_class: tyname.as_str(),
            root_path: &self.root_path(),
            static_root_path: self.shared.static_root_path.as_deref(),
            title: &title,
            description: &desc,
            keywords: &keywords,
            resource_suffix: &self.shared.resource_suffix,
            extra_scripts: &[],
            static_extra_scripts: &[],
        };

        {
            self.id_map.borrow_mut().reset();
            self.id_map.borrow_mut().populate(initial_ids());
        }

        if !self.render_redirect_pages {
            layout::render(&self.shared.layout, &page,
                           |buf: &mut _| print_sidebar(self, it, buf),
                           |buf: &mut _| print_item(self, it, buf),
                           &self.shared.themes)
        } else {
            let mut url = self.root_path();
            if let Some(&(ref names, ty)) = self.cache.paths.get(&it.def_id) {
                for name in &names[..names.len() - 1] {
                    url.push_str(name);
                    url.push_str("/");
                }
                url.push_str(&item_path(ty, names.last().unwrap()));
                layout::redirect(&url)
            } else {
                String::new()
            }
        }
    }

    /// Non-parallelized version of rendering an item. This will take the input
    /// item, render its contents, and then invoke the specified closure with
    /// all sub-items which need to be rendered.
    ///
    /// The rendering driver uses this closure to queue up more work.
    fn item<F>(&mut self, item: clean::Item, all: &mut AllTypes, mut f: F) -> Result<(), Error>
        where F: FnMut(&mut Context, clean::Item),
    {
        // Stripped modules survive the rustdoc passes (i.e., `strip-private`)
        // if they contain impls for public types. These modules can also
        // contain items such as publicly re-exported structures.
        //
        // External crates will provide links to these structures, so
        // these modules are recursed into, but not rendered normally
        // (a flag on the context).
        if !self.render_redirect_pages {
            self.render_redirect_pages = item.is_stripped();
        }

        if item.is_mod() {
            // modules are special because they add a namespace. We also need to
            // recurse into the items of the module as well.
            let name = item.name.as_ref().unwrap().to_string();
            let scx = &self.shared;
            if name.is_empty() {
                panic!("Unexpected empty destination: {:?}", self.current);
            }
            let prev = self.dst.clone();
            self.dst.push(&name);
            self.current.push(name);

            info!("Recursing into {}", self.dst.display());

            let buf = self.render_item(&item, false);
            // buf will be empty if the module is stripped and there is no redirect for it
            if !buf.is_empty() {
                self.shared.ensure_dir(&self.dst)?;
                let joint_dst = self.dst.join("index.html");
                scx.fs.write(&joint_dst, buf.as_bytes())?;
            }

            let m = match item.inner {
                clean::StrippedItem(box clean::ModuleItem(m)) |
                clean::ModuleItem(m) => m,
                _ => unreachable!()
            };

            // Render sidebar-items.js used throughout this module.
            if !self.render_redirect_pages {
                let items = self.build_sidebar_items(&m);
                let js_dst = self.dst.join("sidebar-items.js");
                let v = format!("initSidebarItems({});", as_json(&items));
                scx.fs.write(&js_dst, &v)?;
            }

            for item in m.items {
                f(self, item);
            }

            info!("Recursed; leaving {}", self.dst.display());

            // Go back to where we were at
            self.dst = prev;
            self.current.pop().unwrap();
        } else if item.name.is_some() {
            let buf = self.render_item(&item, true);
            // buf will be empty if the item is stripped and there is no redirect for it
            if !buf.is_empty() {
                let name = item.name.as_ref().unwrap();
                let item_type = item.type_();
                let file_name = &item_path(item_type, name);
                self.shared.ensure_dir(&self.dst)?;
                let joint_dst = self.dst.join(file_name);
                self.shared.fs.write(&joint_dst, buf.as_bytes())?;

                if !self.render_redirect_pages {
                    all.append(full_path(self, &item), &item_type);
                }
                if self.shared.generate_redirect_pages {
                    // Redirect from a sane URL using the namespace to Rustdoc's
                    // URL for the page.
                    let redir_name = format!("{}.{}.html", name, item_type.name_space());
                    let redir_dst = self.dst.join(redir_name);
                    let v = layout::redirect(file_name);
                    self.shared.fs.write(&redir_dst, v.as_bytes())?;
                }
                // If the item is a macro, redirect from the old macro URL (with !)
                // to the new one (without).
                if item_type == ItemType::Macro {
                    let redir_name = format!("{}.{}!.html", item_type, name);
                    let redir_dst = self.dst.join(redir_name);
                    let v = layout::redirect(file_name);
                    self.shared.fs.write(&redir_dst, v.as_bytes())?;
                }
            }
        }
        Ok(())
    }

    fn build_sidebar_items(&self, m: &clean::Module) -> BTreeMap<String, Vec<NameDoc>> {
        // BTreeMap instead of HashMap to get a sorted output
        let mut map: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for item in &m.items {
            if item.is_stripped() { continue }

            let short = item.type_();
            let myname = match item.name {
                None => continue,
                Some(ref s) => s.to_string(),
            };
            let short = short.to_string();
            map.entry(short).or_default()
                .push((myname, Some(plain_summary_line(item.doc_value()))));
        }

        if self.shared.sort_modules_alphabetically {
            for (_, items) in &mut map {
                items.sort();
            }
        }
        map
    }
}

impl Context {
    /// Generates a url appropriate for an `href` attribute back to the source of
    /// this item.
    ///
    /// The url generated, when clicked, will redirect the browser back to the
    /// original source code.
    ///
    /// If `None` is returned, then a source link couldn't be generated. This
    /// may happen, for example, with externally inlined items where the source
    /// of their crate documentation isn't known.
    fn src_href(&self, item: &clean::Item) -> Option<String> {
        let mut root = self.root_path();

        let mut path = String::new();

        // We can safely ignore macros from other libraries
        let file = match item.source.filename {
            FileName::Real(ref path) => path,
            _ => return None,
        };

        let (krate, path) = if item.def_id.is_local() {
            if let Some(path) = self.shared.local_sources.get(file) {
                (&self.shared.layout.krate, path)
            } else {
                return None;
            }
        } else {
            let (krate, src_root) = match *self.cache.extern_locations.get(&item.def_id.krate)? {
                (ref name, ref src, Local) => (name, src),
                (ref name, ref src, Remote(ref s)) => {
                    root = s.to_string();
                    (name, src)
                }
                (_, _, Unknown) => return None,
            };

            sources::clean_path(&src_root, file, false, |component| {
                path.push_str(&component.to_string_lossy());
                path.push('/');
            });
            let mut fname = file.file_name().expect("source has no filename")
                                .to_os_string();
            fname.push(".html");
            path.push_str(&fname.to_string_lossy());
            (krate, &path)
        };

        let lines = if item.source.loline == item.source.hiline {
            item.source.loline.to_string()
        } else {
            format!("{}-{}", item.source.loline, item.source.hiline)
        };
        Some(format!("{root}src/{krate}/{path}#{lines}",
                     root = Escape(&root),
                     krate = krate,
                     path = path,
                     lines = lines))
    }
}

fn wrap_into_docblock<F>(w: &mut Buffer, f: F)
    where F: FnOnce(&mut Buffer)
{
    write!(w, "<div class=\"docblock type-decl hidden-by-usual-hider\">");
    f(w);
    write!(w, "</div>")
}

fn print_item(cx: &Context, item: &clean::Item, buf: &mut Buffer) {
    debug_assert!(!item.is_stripped());
    // Write the breadcrumb trail header for the top
    write!(buf, "<h1 class='fqn'><span class='out-of-band'>");
    if let Some(version) = item.stable_since() {
        write!(buf, "<span class='since' title='Stable since Rust version {0}'>{0}</span>",
                version);
    }
    write!(buf,
            "<span id='render-detail'>\
                <a id=\"toggle-all-docs\" href=\"javascript:void(0)\" \
                    title=\"collapse all docs\">\
                    [<span class='inner'>&#x2212;</span>]\
                </a>\
            </span>");

    // Write `src` tag
    //
    // When this item is part of a `pub use` in a downstream crate, the
    // [src] link in the downstream documentation will actually come back to
    // this page, and this link will be auto-clicked. The `id` attribute is
    // used to find the link to auto-click.
    if cx.shared.include_sources && !item.is_primitive() {
        if let Some(l) = cx.src_href(item) {
            write!(buf, "<a class='srclink' href='{}' title='{}'>[src]</a>",
                    l, "goto source code");
        }
    }

    write!(buf, "</span>"); // out-of-band
    write!(buf, "<span class='in-band'>");
    let name = match item.inner {
        clean::ModuleItem(ref m) => if m.is_crate {
                "Crate "
            } else {
                "Module "
            },
        clean::FunctionItem(..) | clean::ForeignFunctionItem(..) => "Function ",
        clean::TraitItem(..) => "Trait ",
        clean::StructItem(..) => "Struct ",
        clean::UnionItem(..) => "Union ",
        clean::EnumItem(..) => "Enum ",
        clean::TypedefItem(..) => "Type Definition ",
        clean::MacroItem(..) => "Macro ",
        clean::ProcMacroItem(ref mac) => match mac.kind {
            MacroKind::Bang => "Macro ",
            MacroKind::Attr => "Attribute Macro ",
            MacroKind::Derive => "Derive Macro ",
        }
        clean::PrimitiveItem(..) => "Primitive Type ",
        clean::StaticItem(..) | clean::ForeignStaticItem(..) => "Static ",
        clean::ConstantItem(..) => "Constant ",
        clean::ForeignTypeItem => "Foreign Type ",
        clean::KeywordItem(..) => "Keyword ",
        clean::OpaqueTyItem(..) => "Opaque Type ",
        clean::TraitAliasItem(..) => "Trait Alias ",
        _ => {
            // We don't generate pages for any other type.
            unreachable!();
        }
    };
    buf.write_str(name);
    if !item.is_primitive() && !item.is_keyword() {
        let cur = &cx.current;
        let amt = if item.is_mod() { cur.len() - 1 } else { cur.len() };
        for (i, component) in cur.iter().enumerate().take(amt) {
            write!(buf, "<a href='{}index.html'>{}</a>::<wbr>",
                    "../".repeat(cur.len() - i - 1),
                    component);
        }
    }
    write!(buf, "<a class=\"{}\" href=''>{}</a>",
            item.type_(), item.name.as_ref().unwrap());

    write!(buf, "</span></h1>"); // in-band

    match item.inner {
        clean::ModuleItem(ref m) =>
            item_module(buf, cx, item, &m.items),
        clean::FunctionItem(ref f) | clean::ForeignFunctionItem(ref f) =>
            item_function(buf, cx, item, f),
        clean::TraitItem(ref t) => item_trait(buf, cx, item, t),
        clean::StructItem(ref s) => item_struct(buf, cx, item, s),
        clean::UnionItem(ref s) => item_union(buf, cx, item, s),
        clean::EnumItem(ref e) => item_enum(buf, cx, item, e),
        clean::TypedefItem(ref t, _) => item_typedef(buf, cx, item, t),
        clean::MacroItem(ref m) => item_macro(buf, cx, item, m),
        clean::ProcMacroItem(ref m) => item_proc_macro(buf, cx, item, m),
        clean::PrimitiveItem(_) => item_primitive(buf, cx, item),
        clean::StaticItem(ref i) | clean::ForeignStaticItem(ref i) =>
            item_static(buf, cx, item, i),
        clean::ConstantItem(ref c) => item_constant(buf, cx, item, c),
        clean::ForeignTypeItem => item_foreign_type(buf, cx, item),
        clean::KeywordItem(_) => item_keyword(buf, cx, item),
        clean::OpaqueTyItem(ref e, _) => item_opaque_ty(buf, cx, item, e),
        clean::TraitAliasItem(ref ta) => item_trait_alias(buf, cx, item, ta),
        _ => {
            // We don't generate pages for any other type.
            unreachable!();
        }
    }
}

fn item_path(ty: ItemType, name: &str) -> String {
    match ty {
        ItemType::Module => format!("{}index.html", ensure_trailing_slash(name)),
        _ => format!("{}.{}.html", ty, name),
    }
}

fn full_path(cx: &Context, item: &clean::Item) -> String {
    let mut s = cx.current.join("::");
    s.push_str("::");
    s.push_str(item.name.as_ref().unwrap());
    s
}

#[inline]
fn plain_summary_line(s: Option<&str>) -> String {
    let s = s.unwrap_or("");
    // This essentially gets the first paragraph of text in one line.
    let mut line = s.lines()
        .skip_while(|line| line.chars().all(|c| c.is_whitespace()))
        .take_while(|line| line.chars().any(|c| !c.is_whitespace()))
        .fold(String::new(), |mut acc, line| {
            acc.push_str(line);
            acc.push(' ');
            acc
        });
    // remove final whitespace
    line.pop();
    markdown::plain_summary_line(&line[..])
}

fn shorten(s: String) -> String {
    if s.chars().count() > 60 {
        let mut len = 0;
        let mut ret = s.split_whitespace()
                        .take_while(|p| {
                            // + 1 for the added character after the word.
                            len += p.chars().count() + 1;
                            len < 60
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
        ret.push('…');
        ret
    } else {
        s
    }
}

fn document(w: &mut Buffer, cx: &Context, item: &clean::Item) {
    if let Some(ref name) = item.name {
        info!("Documenting {}", name);
    }
    document_stability(w, cx, item, false);
    document_full(w, item, cx, "", false);
}

/// Render md_text as markdown.
fn render_markdown(
    w: &mut Buffer,
    cx: &Context,
    md_text: &str,
    links: Vec<(String, String)>,
    prefix: &str,
    is_hidden: bool,
) {
    let mut ids = cx.id_map.borrow_mut();
    write!(w, "<div class='docblock{}'>{}{}</div>",
           if is_hidden { " hidden" } else { "" },
           prefix,
           Markdown(md_text, &links, &mut ids,
           cx.shared.codes, cx.shared.edition, &cx.shared.playground).to_string())
}

fn document_short(
    w: &mut Buffer,
    cx: &Context,
    item: &clean::Item,
    link: AssocItemLink<'_>,
    prefix: &str,
    is_hidden: bool,
) {
    if let Some(s) = item.doc_value() {
        let markdown = if s.contains('\n') {
            format!("{} [Read more]({})",
                    &plain_summary_line(Some(s)), naive_assoc_href(item, link))
        } else {
            plain_summary_line(Some(s))
        };
        render_markdown(w, cx, &markdown, item.links(), prefix, is_hidden);
    } else if !prefix.is_empty() {
        write!(w, "<div class='docblock{}'>{}</div>",
               if is_hidden { " hidden" } else { "" },
               prefix);
    }
}

fn document_full(w: &mut Buffer, item: &clean::Item, cx: &Context, prefix: &str, is_hidden: bool) {
    if let Some(s) = cx.shared.maybe_collapsed_doc_value(item) {
        debug!("Doc block: =====\n{}\n=====", s);
        render_markdown(w, cx, &*s, item.links(), prefix, is_hidden);
    } else if !prefix.is_empty() {
        write!(w, "<div class='docblock{}'>{}</div>",
               if is_hidden { " hidden" } else { "" },
               prefix);
    }
}

fn document_stability(w: &mut Buffer, cx: &Context, item: &clean::Item, is_hidden: bool) {
    let stabilities = short_stability(item, cx);
    if !stabilities.is_empty() {
        write!(w, "<div class='stability{}'>", if is_hidden { " hidden" } else { "" });
        for stability in stabilities {
            write!(w, "{}", stability);
        }
        write!(w, "</div>");
    }
}

fn document_non_exhaustive_header(item: &clean::Item) -> &str {
    if item.is_non_exhaustive() { " (Non-exhaustive)" } else { "" }
}

fn document_non_exhaustive(w: &mut Buffer, item: &clean::Item) {
    if item.is_non_exhaustive() {
        write!(w, "<div class='docblock non-exhaustive non-exhaustive-{}'>", {
            if item.is_struct() {
                "struct"
            } else if item.is_enum() {
                "enum"
            } else if item.is_variant() {
                "variant"
            } else {
                "type"
            }
        });

        if item.is_struct() {
            write!(w, "Non-exhaustive structs could have additional fields added in future. \
                       Therefore, non-exhaustive structs cannot be constructed in external crates \
                       using the traditional <code>Struct {{ .. }}</code> syntax; cannot be \
                       matched against without a wildcard <code>..</code>; and \
                       struct update syntax will not work.");
        } else if item.is_enum() {
            write!(w, "Non-exhaustive enums could have additional variants added in future. \
                       Therefore, when matching against variants of non-exhaustive enums, an \
                       extra wildcard arm must be added to account for any future variants.");
        } else if item.is_variant() {
            write!(w, "Non-exhaustive enum variants could have additional fields added in future. \
                       Therefore, non-exhaustive enum variants cannot be constructed in external \
                       crates and cannot be matched against.");
        } else {
            write!(w, "This type will require a wildcard arm in any match statements or \
                       constructors.");
        }

        write!(w, "</div>");
    }
}

fn name_key(name: &str) -> (&str, u64, usize) {
    let end = name.bytes()
        .rposition(|b| b.is_ascii_digit()).map_or(name.len(), |i| i + 1);

    // find number at end
    let split = name[0..end].bytes()
        .rposition(|b| !b.is_ascii_digit()).map_or(0, |i| i + 1);

    // count leading zeroes
    let after_zeroes =
        name[split..end].bytes().position(|b| b != b'0').map_or(name.len(), |extra| split + extra);

    // sort leading zeroes last
    let num_zeroes = after_zeroes - split;

    match name[split..end].parse() {
        Ok(n) => (&name[..split], n, num_zeroes),
        Err(_) => (name, 0, num_zeroes),
    }
}

fn item_module(w: &mut Buffer, cx: &Context, item: &clean::Item, items: &[clean::Item]) {
    document(w, cx, item);

    let mut indices = (0..items.len()).filter(|i| !items[*i].is_stripped()).collect::<Vec<usize>>();

    // the order of item types in the listing
    fn reorder(ty: ItemType) -> u8 {
        match ty {
            ItemType::ExternCrate     => 0,
            ItemType::Import          => 1,
            ItemType::Primitive       => 2,
            ItemType::Module          => 3,
            ItemType::Macro           => 4,
            ItemType::Struct          => 5,
            ItemType::Enum            => 6,
            ItemType::Constant        => 7,
            ItemType::Static          => 8,
            ItemType::Trait           => 9,
            ItemType::Function        => 10,
            ItemType::Typedef         => 12,
            ItemType::Union           => 13,
            _                         => 14 + ty as u8,
        }
    }

    fn cmp(i1: &clean::Item, i2: &clean::Item, idx1: usize, idx2: usize) -> Ordering {
        let ty1 = i1.type_();
        let ty2 = i2.type_();
        if ty1 != ty2 {
            return (reorder(ty1), idx1).cmp(&(reorder(ty2), idx2))
        }
        let s1 = i1.stability.as_ref().map(|s| s.level);
        let s2 = i2.stability.as_ref().map(|s| s.level);
        match (s1, s2) {
            (Some(stability::Unstable), Some(stability::Stable)) => return Ordering::Greater,
            (Some(stability::Stable), Some(stability::Unstable)) => return Ordering::Less,
            _ => {}
        }
        let lhs = i1.name.as_ref().map_or("", |s| &**s);
        let rhs = i2.name.as_ref().map_or("", |s| &**s);
        name_key(lhs).cmp(&name_key(rhs))
    }

    if cx.shared.sort_modules_alphabetically {
        indices.sort_by(|&i1, &i2| cmp(&items[i1], &items[i2], i1, i2));
    }
    // This call is to remove re-export duplicates in cases such as:
    //
    // ```
    // pub mod foo {
    //     pub mod bar {
    //         pub trait Double { fn foo(); }
    //     }
    // }
    //
    // pub use foo::bar::*;
    // pub use foo::*;
    // ```
    //
    // `Double` will appear twice in the generated docs.
    //
    // FIXME: This code is quite ugly and could be improved. Small issue: DefId
    // can be identical even if the elements are different (mostly in imports).
    // So in case this is an import, we keep everything by adding a "unique id"
    // (which is the position in the vector).
    indices.dedup_by_key(|i| (items[*i].def_id,
                              if items[*i].name.as_ref().is_some() {
                                  Some(full_path(cx, &items[*i]))
                              } else {
                                  None
                              },
                              items[*i].type_(),
                              if items[*i].is_import() {
                                  *i
                              } else {
                                  0
                              }));

    debug!("{:?}", indices);
    let mut curty = None;
    for &idx in &indices {
        let myitem = &items[idx];
        if myitem.is_stripped() {
            continue;
        }

        let myty = Some(myitem.type_());
        if curty == Some(ItemType::ExternCrate) && myty == Some(ItemType::Import) {
            // Put `extern crate` and `use` re-exports in the same section.
            curty = myty;
        } else if myty != curty {
            if curty.is_some() {
                write!(w, "</table>");
            }
            curty = myty;
            let (short, name) = item_ty_to_strs(&myty.unwrap());
            write!(w, "<h2 id='{id}' class='section-header'>\
                       <a href=\"#{id}\">{name}</a></h2>\n<table>",
                   id = cx.derive_id(short.to_owned()), name = name);
        }

        match myitem.inner {
            clean::ExternCrateItem(ref name, ref src) => {
                use crate::html::format::anchor;

                match *src {
                    Some(ref src) => {
                        write!(w, "<tr><td><code>{}extern crate {} as {};",
                               myitem.visibility.print_with_space(),
                               anchor(myitem.def_id, src),
                               name)
                    }
                    None => {
                        write!(w, "<tr><td><code>{}extern crate {};",
                               myitem.visibility.print_with_space(),
                               anchor(myitem.def_id, name))
                    }
                }
                write!(w, "</code></td></tr>");
            }

            clean::ImportItem(ref import) => {
                write!(w, "<tr><td><code>{}{}</code></td></tr>",
                       myitem.visibility.print_with_space(), import.print());
            }

            _ => {
                if myitem.name.is_none() { continue }

                let unsafety_flag = match myitem.inner {
                    clean::FunctionItem(ref func) | clean::ForeignFunctionItem(ref func)
                    if func.header.unsafety == hir::Unsafety::Unsafe => {
                        "<a title='unsafe function' href='#'><sup>⚠</sup></a>"
                    }
                    _ => "",
                };

                let stab = myitem.stability_class();
                let add = if stab.is_some() {
                    " "
                } else {
                    ""
                };

                let doc_value = myitem.doc_value().unwrap_or("");
                write!(w, "\
                       <tr class='{stab}{add}module-item'>\
                           <td><a class=\"{class}\" href=\"{href}\" \
                                  title='{title}'>{name}</a>{unsafety_flag}</td>\
                           <td class='docblock-short'>{stab_tags}{docs}</td>\
                       </tr>",
                       name = *myitem.name.as_ref().unwrap(),
                       stab_tags = stability_tags(myitem),
                       docs = MarkdownSummaryLine(doc_value, &myitem.links()).to_string(),
                       class = myitem.type_(),
                       add = add,
                       stab = stab.unwrap_or_else(|| String::new()),
                       unsafety_flag = unsafety_flag,
                       href = item_path(myitem.type_(), myitem.name.as_ref().unwrap()),
                       title = [full_path(cx, myitem), myitem.type_().to_string()]
                                .iter()
                                .filter_map(|s| if !s.is_empty() {
                                    Some(s.as_str())
                                } else {
                                    None
                                })
                                .collect::<Vec<_>>()
                                .join(" "),
                      );
            }
        }
    }

    if curty.is_some() {
        write!(w, "</table>");
    }
}

/// Render the stability and deprecation tags that are displayed in the item's summary at the
/// module level.
fn stability_tags(item: &clean::Item) -> String {
    let mut tags = String::new();

    fn tag_html(class: &str, contents: &str) -> String {
        format!(r#"<span class="stab {}">{}</span>"#, class, contents)
    }

    // The trailing space after each tag is to space it properly against the rest of the docs.
    if item.deprecation().is_some() {
        let mut message = "Deprecated";
        if let Some(ref stab) = item.stability {
            if let Some(ref depr) = stab.deprecation {
                if let Some(ref since) = depr.since {
                    if !stability::deprecation_in_effect(&since) {
                        message = "Deprecation planned";
                    }
                }
            }
        }
        tags += &tag_html("deprecated", message);
    }

    if let Some(stab) = item
        .stability
        .as_ref()
        .filter(|s| s.level == stability::Unstable)
    {
        if stab.feature.as_ref().map(|s| &**s) == Some("rustc_private") {
            tags += &tag_html("internal", "Internal");
        } else {
            tags += &tag_html("unstable", "Experimental");
        }
    }

    if let Some(ref cfg) = item.attrs.cfg {
        tags += &tag_html("portability", &cfg.render_short_html());
    }

    tags
}

/// Render the stability and/or deprecation warning that is displayed at the top of the item's
/// documentation.
fn short_stability(item: &clean::Item, cx: &Context) -> Vec<String> {
    let mut stability = vec![];
    let error_codes = cx.shared.codes;

    if let Some(Deprecation { note, since }) = &item.deprecation() {
        // We display deprecation messages for #[deprecated] and #[rustc_deprecated]
        // but only display the future-deprecation messages for #[rustc_deprecated].
        let mut message = if let Some(since) = since {
            format!("Deprecated since {}", Escape(since))
        } else {
            String::from("Deprecated")
        };
        if let Some(ref stab) = item.stability {
            if let Some(ref depr) = stab.deprecation {
                if let Some(ref since) = depr.since {
                    if !stability::deprecation_in_effect(&since) {
                        message = format!("Deprecating in {}", Escape(&since));
                    }
                }
            }
        }

        if let Some(note) = note {
            let mut ids = cx.id_map.borrow_mut();
            let html = MarkdownHtml(
                &note, &mut ids, error_codes, cx.shared.edition, &cx.shared.playground);
            message.push_str(&format!(": {}", html.to_string()));
        }
        stability.push(format!("<div class='stab deprecated'>{}</div>", message));
    }

    if let Some(stab) = item
        .stability
        .as_ref()
        .filter(|stab| stab.level == stability::Unstable)
    {
        let is_rustc_private = stab.feature.as_ref().map(|s| &**s) == Some("rustc_private");

        let mut message = if is_rustc_private {
            "<span class='emoji'>⚙️</span> This is an internal compiler API."
        } else {
            "<span class='emoji'>🔬</span> This is a nightly-only experimental API."
        }
        .to_owned();

        if let Some(feature) = stab.feature.as_ref() {
            let mut feature = format!("<code>{}</code>", Escape(&feature));
            if let (Some(url), Some(issue)) = (&cx.shared.issue_tracker_base_url, stab.issue) {
                feature.push_str(&format!(
                    "&nbsp;<a href=\"{url}{issue}\">#{issue}</a>",
                    url = url,
                    issue = issue
                ));
            }

            message.push_str(&format!(" ({})", feature));
        }

        if let Some(unstable_reason) = &stab.unstable_reason {
            // Provide a more informative message than the compiler help.
            let unstable_reason = if is_rustc_private {
                "This crate is being loaded from the sysroot, a permanently unstable location \
                for private compiler dependencies. It is not intended for general use. Prefer \
                using a public version of this crate from \
                [crates.io](https://crates.io) via [`Cargo.toml`]\
                (https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)."
            } else {
                unstable_reason
            };

            let mut ids = cx.id_map.borrow_mut();
            message = format!(
                "<details><summary>{}</summary>{}</details>",
                message,
                MarkdownHtml(
                    &unstable_reason,
                    &mut ids,
                    error_codes,
                    cx.shared.edition,
                    &cx.shared.playground,
                ).to_string()
            );
        }

        let class = if is_rustc_private {
            "internal"
        } else {
            "unstable"
        };
        stability.push(format!("<div class='stab {}'>{}</div>", class, message));
    }

    if let Some(ref cfg) = item.attrs.cfg {
        stability.push(format!(
            "<div class='stab portability'>{}</div>",
            cfg.render_long_html()
        ));
    }

    stability
}

fn item_constant(w: &mut Buffer, cx: &Context, it: &clean::Item, c: &clean::Constant) {
    write!(w, "<pre class='rust const'>");
    render_attributes(w, it, false);
    write!(w, "{vis}const \
               {name}: {typ}</pre>",
           vis = it.visibility.print_with_space(),
           name = it.name.as_ref().unwrap(),
           typ = c.type_.print());
    document(w, cx, it)
}

fn item_static(w: &mut Buffer, cx: &Context, it: &clean::Item, s: &clean::Static) {
    write!(w, "<pre class='rust static'>");
    render_attributes(w, it, false);
    write!(w, "{vis}static {mutability}\
               {name}: {typ}</pre>",
           vis = it.visibility.print_with_space(),
           mutability = s.mutability.print_with_space(),
           name = it.name.as_ref().unwrap(),
           typ = s.type_.print());
    document(w, cx, it)
}

fn item_function(w: &mut Buffer, cx: &Context, it: &clean::Item, f: &clean::Function) {
    let header_len = format!(
        "{}{}{}{}{:#}fn {}{:#}",
        it.visibility.print_with_space(),
        f.header.constness.print_with_space(),
        f.header.unsafety.print_with_space(),
        f.header.asyncness.print_with_space(),
        print_abi_with_space(f.header.abi),
        it.name.as_ref().unwrap(),
        f.generics.print()
    ).len();
    write!(w, "{}<pre class='rust fn'>", render_spotlight_traits(it));
    render_attributes(w, it, false);
    write!(w,
           "{vis}{constness}{unsafety}{asyncness}{abi}fn \
           {name}{generics}{decl}{where_clause}</pre>",
           vis = it.visibility.print_with_space(),
           constness = f.header.constness.print_with_space(),
           unsafety = f.header.unsafety.print_with_space(),
           asyncness = f.header.asyncness.print_with_space(),
           abi = print_abi_with_space(f.header.abi),
           name = it.name.as_ref().unwrap(),
           generics = f.generics.print(),
           where_clause = WhereClause { gens: &f.generics, indent: 0, end_newline: true },
           decl = Function {
              decl: &f.decl,
              header_len,
              indent: 0,
              asyncness: f.header.asyncness,
           }.print());
    document(w, cx, it)
}

fn render_implementor(cx: &Context, implementor: &Impl, w: &mut Buffer,
                      implementor_dups: &FxHashMap<&str, (DefId, bool)>) {
    // If there's already another implementor that has the same abbridged name, use the
    // full path, for example in `std::iter::ExactSizeIterator`
    let use_absolute = match implementor.inner_impl().for_ {
        clean::ResolvedPath { ref path, is_generic: false, .. } |
        clean::BorrowedRef {
            type_: box clean::ResolvedPath { ref path, is_generic: false, .. },
            ..
        } => implementor_dups[path.last_name()].1,
        _ => false,
    };
    render_impl(w, cx, implementor, AssocItemLink::Anchor(None), RenderMode::Normal,
                implementor.impl_item.stable_since(), false, Some(use_absolute), false, false);
}

fn render_impls(cx: &Context, w: &mut Buffer,
                traits: &[&&Impl],
                containing_item: &clean::Item) {
    for i in traits {
        let did = i.trait_did().unwrap();
        let assoc_link = AssocItemLink::GotoSource(did, &i.inner_impl().provided_trait_methods);
        render_impl(w, cx, i, assoc_link,
                    RenderMode::Normal, containing_item.stable_since(), true, None, false, true);
    }
}

fn bounds(t_bounds: &[clean::GenericBound], trait_alias: bool) -> String {
    let mut bounds = String::new();
    if !t_bounds.is_empty() {
        if !trait_alias {
            bounds.push_str(": ");
        }
        for (i, p) in t_bounds.iter().enumerate() {
            if i > 0 {
                bounds.push_str(" + ");
            }
            bounds.push_str(&p.print().to_string());
        }
    }
    bounds
}

fn compare_impl<'a, 'b>(lhs: &'a &&Impl, rhs: &'b &&Impl) -> Ordering {
    let lhs = format!("{}", lhs.inner_impl().print());
    let rhs = format!("{}", rhs.inner_impl().print());

    // lhs and rhs are formatted as HTML, which may be unnecessary
    name_key(&lhs).cmp(&name_key(&rhs))
}

fn item_trait(
    w: &mut Buffer,
    cx: &Context,
    it: &clean::Item,
    t: &clean::Trait,
) {
    let bounds = bounds(&t.bounds, false);
    let types = t.items.iter().filter(|m| m.is_associated_type()).collect::<Vec<_>>();
    let consts = t.items.iter().filter(|m| m.is_associated_const()).collect::<Vec<_>>();
    let required = t.items.iter().filter(|m| m.is_ty_method()).collect::<Vec<_>>();
    let provided = t.items.iter().filter(|m| m.is_method()).collect::<Vec<_>>();

    // Output the trait definition
    wrap_into_docblock(w, |w| {
        write!(w, "<pre class='rust trait'>");
        render_attributes(w, it, true);
        write!(w, "{}{}{}trait {}{}{}",
               it.visibility.print_with_space(),
               t.unsafety.print_with_space(),
               if t.is_auto { "auto " } else { "" },
               it.name.as_ref().unwrap(),
               t.generics.print(),
               bounds);

        if !t.generics.where_predicates.is_empty() {
            write!(w, "{}", WhereClause { gens: &t.generics, indent: 0, end_newline: true });
        } else {
            write!(w, " ");
        }

        if t.items.is_empty() {
            write!(w, "{{ }}");
        } else {
            // FIXME: we should be using a derived_id for the Anchors here
            write!(w, "{{\n");
            for t in &types {
                render_assoc_item(w, t, AssocItemLink::Anchor(None), ItemType::Trait);
                write!(w, ";\n");
            }
            if !types.is_empty() && !consts.is_empty() {
                w.write_str("\n");
            }
            for t in &consts {
                render_assoc_item(w, t, AssocItemLink::Anchor(None), ItemType::Trait);
                write!(w, ";\n");
            }
            if !consts.is_empty() && !required.is_empty() {
                w.write_str("\n");
            }
            for (pos, m) in required.iter().enumerate() {
                render_assoc_item(w, m, AssocItemLink::Anchor(None), ItemType::Trait);
                write!(w, ";\n");

                if pos < required.len() - 1 {
                   write!(w, "<div class='item-spacer'></div>");
                }
            }
            if !required.is_empty() && !provided.is_empty() {
                w.write_str("\n");
            }
            for (pos, m) in provided.iter().enumerate() {
                render_assoc_item(w, m, AssocItemLink::Anchor(None), ItemType::Trait);
                match m.inner {
                    clean::MethodItem(ref inner) if !inner.generics.where_predicates.is_empty() => {
                        write!(w, ",\n    {{ ... }}\n");
                    },
                    _ => {
                        write!(w, " {{ ... }}\n");
                    },
                }
                if pos < provided.len() - 1 {
                   write!(w, "<div class='item-spacer'></div>");
                }
            }
            write!(w, "}}");
        }
        write!(w, "</pre>")
    });

    // Trait documentation
    document(w, cx, it);

    fn write_small_section_header(
        w: &mut Buffer,
        id: &str,
        title: &str,
        extra_content: &str,
    ) {
        write!(w, "
            <h2 id='{0}' class='small-section-header'>\
              {1}<a href='#{0}' class='anchor'></a>\
            </h2>{2}", id, title, extra_content)
    }

    fn write_loading_content(w: &mut Buffer, extra_content: &str) {
        write!(w, "{}<span class='loading-content'>Loading content...</span>", extra_content)
    }

    fn trait_item(w: &mut Buffer, cx: &Context, m: &clean::Item, t: &clean::Item) {
        let name = m.name.as_ref().unwrap();
        let item_type = m.type_();
        let id = cx.derive_id(format!("{}.{}", item_type, name));
        let ns_id = cx.derive_id(format!("{}.{}", name, item_type.name_space()));
        write!(w, "<h3 id='{id}' class='method'>{extra}<code id='{ns_id}'>",
               extra = render_spotlight_traits(m),
               id = id,
               ns_id = ns_id);
        render_assoc_item(w, m, AssocItemLink::Anchor(Some(&id)), ItemType::Impl);
        write!(w, "</code>");
        render_stability_since(w, m, t);
        write!(w, "</h3>");
        document(w, cx, m);
    }

    if !types.is_empty() {
        write_small_section_header(w, "associated-types", "Associated Types",
                                   "<div class='methods'>");
        for t in &types {
            trait_item(w, cx, *t, it);
        }
        write_loading_content(w, "</div>");
    }

    if !consts.is_empty() {
        write_small_section_header(w, "associated-const", "Associated Constants",
                                   "<div class='methods'>");
        for t in &consts {
            trait_item(w, cx, *t, it);
        }
        write_loading_content(w, "</div>");
    }

    // Output the documentation for each function individually
    if !required.is_empty() {
        write_small_section_header(w, "required-methods", "Required methods",
                                   "<div class='methods'>");
        for m in &required {
            trait_item(w, cx, *m, it);
        }
        write_loading_content(w, "</div>");
    }
    if !provided.is_empty() {
        write_small_section_header(w, "provided-methods", "Provided methods",
                                   "<div class='methods'>");
        for m in &provided {
            trait_item(w, cx, *m, it);
        }
        write_loading_content(w, "</div>");
    }

    // If there are methods directly on this trait object, render them here.
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All);

    let mut synthetic_types = Vec::new();

    if let Some(implementors) = cx.cache.implementors.get(&it.def_id) {
        // The DefId is for the first Type found with that name. The bool is
        // if any Types with the same name but different DefId have been found.
        let mut implementor_dups: FxHashMap<&str, (DefId, bool)> = FxHashMap::default();
        for implementor in implementors {
            match implementor.inner_impl().for_ {
                clean::ResolvedPath { ref path, did, is_generic: false, .. } |
                clean::BorrowedRef {
                    type_: box clean::ResolvedPath { ref path, did, is_generic: false, .. },
                    ..
                } => {
                    let &mut (prev_did, ref mut has_duplicates) =
                        implementor_dups.entry(path.last_name()).or_insert((did, false));
                    if prev_did != did {
                        *has_duplicates = true;
                    }
                }
                _ => {}
            }
        }

        let (local, foreign) = implementors.iter()
            .partition::<Vec<_>, _>(|i| i.inner_impl().for_.def_id()
                                         .map_or(true, |d| cx.cache.paths.contains_key(&d)));


        let (mut synthetic, mut concrete): (Vec<&&Impl>, Vec<&&Impl>) = local.iter()
            .partition(|i| i.inner_impl().synthetic);

        synthetic.sort_by(compare_impl);
        concrete.sort_by(compare_impl);

        if !foreign.is_empty() {
            write_small_section_header(w, "foreign-impls", "Implementations on Foreign Types", "");

            for implementor in foreign {
                let assoc_link = AssocItemLink::GotoSource(
                    implementor.impl_item.def_id,
                    &implementor.inner_impl().provided_trait_methods
                );
                render_impl(w, cx, &implementor, assoc_link,
                            RenderMode::Normal, implementor.impl_item.stable_since(), false,
                            None, true, false);
            }
            write_loading_content(w, "");
        }

        write_small_section_header(w, "implementors", "Implementors",
                                   "<div class='item-list' id='implementors-list'>");
        for implementor in concrete {
            render_implementor(cx, implementor, w, &implementor_dups);
        }
        write_loading_content(w, "</div>");

        if t.auto {
            write_small_section_header(w, "synthetic-implementors", "Auto implementors",
                                       "<div class='item-list' id='synthetic-implementors-list'>");
            for implementor in synthetic {
                synthetic_types.extend(
                    collect_paths_for_type(implementor.inner_impl().for_.clone())
                );
                render_implementor(cx, implementor, w, &implementor_dups);
            }
            write_loading_content(w, "</div>");
        }
    } else {
        // even without any implementations to write in, we still want the heading and list, so the
        // implementors javascript file pulled in below has somewhere to write the impls into
        write_small_section_header(w, "implementors", "Implementors",
                                   "<div class='item-list' id='implementors-list'>");
        write_loading_content(w, "</div>");

        if t.auto {
            write_small_section_header(w, "synthetic-implementors", "Auto implementors",
                                       "<div class='item-list' id='synthetic-implementors-list'>");
            write_loading_content(w, "</div>");
        }
    }
    write!(w, r#"<script type="text/javascript">window.inlined_types=new Set({});</script>"#,
           as_json(&synthetic_types));

    write!(w, r#"<script type="text/javascript" async
                         src="{root_path}/implementors/{path}/{ty}.{name}.js">
                 </script>"#,
           root_path = vec![".."; cx.current.len()].join("/"),
           path = if it.def_id.is_local() {
               cx.current.join("/")
           } else {
               let (ref path, _) = cx.cache.external_paths[&it.def_id];
               path[..path.len() - 1].join("/")
           },
           ty = it.type_(),
           name = *it.name.as_ref().unwrap());
}

fn naive_assoc_href(it: &clean::Item, link: AssocItemLink<'_>) -> String {
    use crate::html::item_type::ItemType::*;

    let name = it.name.as_ref().unwrap();
    let ty = match it.type_() {
        Typedef | AssocType => AssocType,
        s@_ => s,
    };

    let anchor = format!("#{}.{}", ty, name);
    match link {
        AssocItemLink::Anchor(Some(ref id)) => format!("#{}", id),
        AssocItemLink::Anchor(None) => anchor,
        AssocItemLink::GotoSource(did, _) => {
            href(did).map(|p| format!("{}{}", p.0, anchor)).unwrap_or(anchor)
        }
    }
}

fn assoc_const(w: &mut Buffer,
               it: &clean::Item,
               ty: &clean::Type,
               _default: Option<&String>,
               link: AssocItemLink<'_>,
               extra: &str) {
    write!(w, "{}{}const <a href='{}' class=\"constant\"><b>{}</b></a>: {}",
           extra,
           it.visibility.print_with_space(),
           naive_assoc_href(it, link),
           it.name.as_ref().unwrap(),
           ty.print());
}

fn assoc_type(w: &mut Buffer, it: &clean::Item,
              bounds: &[clean::GenericBound],
              default: Option<&clean::Type>,
              link: AssocItemLink<'_>,
              extra: &str) {
    write!(w, "{}type <a href='{}' class=\"type\">{}</a>",
           extra,
           naive_assoc_href(it, link),
           it.name.as_ref().unwrap());
    if !bounds.is_empty() {
        write!(w, ": {}", print_generic_bounds(bounds))
    }
    if let Some(default) = default {
        write!(w, " = {}", default.print())
    }
}

fn render_stability_since_raw(w: &mut Buffer, ver: Option<&str>, containing_ver: Option<&str>) {
    if let Some(v) = ver {
        if containing_ver != ver && v.len() > 0 {
            write!(w, "<span class='since' title='Stable since Rust version {0}'>{0}</span>", v)
        }
    }
}

fn render_stability_since(w: &mut Buffer, item: &clean::Item, containing_item: &clean::Item) {
    render_stability_since_raw(w, item.stable_since(), containing_item.stable_since())
}

fn render_assoc_item(w: &mut Buffer,
                     item: &clean::Item,
                     link: AssocItemLink<'_>,
                     parent: ItemType) {
    fn method(w: &mut Buffer,
              meth: &clean::Item,
              header: hir::FnHeader,
              g: &clean::Generics,
              d: &clean::FnDecl,
              link: AssocItemLink<'_>,
              parent: ItemType) {
        let name = meth.name.as_ref().unwrap();
        let anchor = format!("#{}.{}", meth.type_(), name);
        let href = match link {
            AssocItemLink::Anchor(Some(ref id)) => format!("#{}", id),
            AssocItemLink::Anchor(None) => anchor,
            AssocItemLink::GotoSource(did, provided_methods) => {
                // We're creating a link from an impl-item to the corresponding
                // trait-item and need to map the anchored type accordingly.
                let ty = if provided_methods.contains(name) {
                    ItemType::Method
                } else {
                    ItemType::TyMethod
                };

                href(did).map(|p| format!("{}#{}.{}", p.0, ty, name)).unwrap_or(anchor)
            }
        };
        let mut header_len = format!(
            "{}{}{}{}{}{:#}fn {}{:#}",
            meth.visibility.print_with_space(),
            header.constness.print_with_space(),
            header.unsafety.print_with_space(),
            header.asyncness.print_with_space(),
            print_default_space(meth.is_default()),
            print_abi_with_space(header.abi),
            name,
            g.print()
        ).len();
        let (indent, end_newline) = if parent == ItemType::Trait {
            header_len += 4;
            (4, false)
        } else {
            (0, true)
        };
        render_attributes(w, meth, false);
        write!(w, "{}{}{}{}{}{}{}fn <a href='{href}' class='fnname'>{name}</a>\
                   {generics}{decl}{where_clause}",
               if parent == ItemType::Trait { "    " } else { "" },
               meth.visibility.print_with_space(),
               header.constness.print_with_space(),
               header.unsafety.print_with_space(),
               header.asyncness.print_with_space(),
               print_default_space(meth.is_default()),
               print_abi_with_space(header.abi),
               href = href,
               name = name,
               generics = g.print(),
               decl = Function {
                   decl: d,
                   header_len,
                   indent,
                   asyncness: header.asyncness,
               }.print(),
               where_clause = WhereClause {
                   gens: g,
                   indent,
                   end_newline,
               })
    }
    match item.inner {
        clean::StrippedItem(..) => {},
        clean::TyMethodItem(ref m) => {
            method(w, item, m.header, &m.generics, &m.decl, link, parent)
        }
        clean::MethodItem(ref m) => {
            method(w, item, m.header, &m.generics, &m.decl, link, parent)
        }
        clean::AssocConstItem(ref ty, ref default) => {
            assoc_const(w, item, ty, default.as_ref(), link,
                        if parent == ItemType::Trait { "    " } else { "" })
        }
        clean::AssocTypeItem(ref bounds, ref default) => {
            assoc_type(w, item, bounds, default.as_ref(), link,
                       if parent == ItemType::Trait { "    " } else { "" })
        }
        _ => panic!("render_assoc_item called on non-associated-item")
    }
}

fn item_struct(w: &mut Buffer, cx: &Context, it: &clean::Item, s: &clean::Struct) {
    wrap_into_docblock(w, |w| {
        write!(w, "<pre class='rust struct'>");
        render_attributes(w, it, true);
        render_struct(w,
                      it,
                      Some(&s.generics),
                      s.struct_type,
                      &s.fields,
                      "",
                      true);
        write!(w, "</pre>")
    });

    document(w, cx, it);
    let mut fields = s.fields.iter().filter_map(|f| {
        match f.inner {
            clean::StructFieldItem(ref ty) => Some((f, ty)),
            _ => None,
        }
    }).peekable();
    if let doctree::Plain = s.struct_type {
        if fields.peek().is_some() {
            write!(w, "<h2 id='fields' class='fields small-section-header'>
                       Fields{}<a href='#fields' class='anchor'></a></h2>",
                       document_non_exhaustive_header(it));
            document_non_exhaustive(w, it);
            for (field, ty) in fields {
                let id = cx.derive_id(format!("{}.{}",
                                           ItemType::StructField,
                                           field.name.as_ref().unwrap()));
                let ns_id = cx.derive_id(format!("{}.{}",
                                              field.name.as_ref().unwrap(),
                                              ItemType::StructField.name_space()));
                write!(w, "<span id=\"{id}\" class=\"{item_type} small-section-header\">\
                           <a href=\"#{id}\" class=\"anchor field\"></a>\
                           <code id=\"{ns_id}\">{name}: {ty}</code>\
                           </span>",
                       item_type = ItemType::StructField,
                       id = id,
                       ns_id = ns_id,
                       name = field.name.as_ref().unwrap(),
                       ty = ty.print());
                document(w, cx, field);
            }
        }
    }
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_union(w: &mut Buffer, cx: &Context, it: &clean::Item, s: &clean::Union) {
    wrap_into_docblock(w, |w| {
        write!(w, "<pre class='rust union'>");
        render_attributes(w, it, true);
        render_union(w,
                     it,
                     Some(&s.generics),
                     &s.fields,
                     "",
                     true);
        write!(w, "</pre>")
    });

    document(w, cx, it);
    let mut fields = s.fields.iter().filter_map(|f| {
        match f.inner {
            clean::StructFieldItem(ref ty) => Some((f, ty)),
            _ => None,
        }
    }).peekable();
    if fields.peek().is_some() {
        write!(w, "<h2 id='fields' class='fields small-section-header'>
                   Fields<a href='#fields' class='anchor'></a></h2>");
        for (field, ty) in fields {
            let name = field.name.as_ref().expect("union field name");
            let id = format!("{}.{}", ItemType::StructField, name);
            write!(w, "<span id=\"{id}\" class=\"{shortty} small-section-header\">\
                           <a href=\"#{id}\" class=\"anchor field\"></a>\
                           <code>{name}: {ty}</code>\
                       </span>",
                   id = id,
                   name = name,
                   shortty = ItemType::StructField,
                   ty = ty.print());
            if let Some(stability_class) = field.stability_class() {
                write!(w, "<span class='stab {stab}'></span>",
                    stab = stability_class);
            }
            document(w, cx, field);
        }
    }
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_enum(w: &mut Buffer, cx: &Context, it: &clean::Item, e: &clean::Enum) {
    wrap_into_docblock(w, |w| {
        write!(w, "<pre class='rust enum'>");
        render_attributes(w, it, true);
        write!(w, "{}enum {}{}{}",
               it.visibility.print_with_space(),
               it.name.as_ref().unwrap(),
               e.generics.print(),
               WhereClause { gens: &e.generics, indent: 0, end_newline: true });
        if e.variants.is_empty() && !e.variants_stripped {
            write!(w, " {{}}");
        } else {
            write!(w, " {{\n");
            for v in &e.variants {
                write!(w, "    ");
                let name = v.name.as_ref().unwrap();
                match v.inner {
                    clean::VariantItem(ref var) => {
                        match var.kind {
                            clean::VariantKind::CLike => write!(w, "{}", name),
                            clean::VariantKind::Tuple(ref tys) => {
                                write!(w, "{}(", name);
                                for (i, ty) in tys.iter().enumerate() {
                                    if i > 0 {
                                        write!(w, ",&nbsp;")
                                    }
                                    write!(w, "{}", ty.print());
                                }
                                write!(w, ")");
                            }
                            clean::VariantKind::Struct(ref s) => {
                                render_struct(w,
                                              v,
                                              None,
                                              s.struct_type,
                                              &s.fields,
                                              "    ",
                                              false);
                            }
                        }
                    }
                    _ => unreachable!()
                }
                write!(w, ",\n");
            }

            if e.variants_stripped {
                write!(w, "    // some variants omitted\n");
            }
            write!(w, "}}");
        }
        write!(w, "</pre>")
    });

    document(w, cx, it);
    if !e.variants.is_empty() {
        write!(w, "<h2 id='variants' class='variants small-section-header'>
                   Variants{}<a href='#variants' class='anchor'></a></h2>\n",
                   document_non_exhaustive_header(it));
        document_non_exhaustive(w, it);
        for variant in &e.variants {
            let id = cx.derive_id(format!("{}.{}",
                                       ItemType::Variant,
                                       variant.name.as_ref().unwrap()));
            let ns_id = cx.derive_id(format!("{}.{}",
                                          variant.name.as_ref().unwrap(),
                                          ItemType::Variant.name_space()));
            write!(w, "<div id=\"{id}\" class=\"variant small-section-header\">\
                       <a href=\"#{id}\" class=\"anchor field\"></a>\
                       <code id='{ns_id}'>{name}",
                   id = id,
                   ns_id = ns_id,
                   name = variant.name.as_ref().unwrap());
            if let clean::VariantItem(ref var) = variant.inner {
                if let clean::VariantKind::Tuple(ref tys) = var.kind {
                    write!(w, "(");
                    for (i, ty) in tys.iter().enumerate() {
                        if i > 0 {
                            write!(w, ",&nbsp;");
                        }
                        write!(w, "{}", ty.print());
                    }
                    write!(w, ")");
                }
            }
            write!(w, "</code></div>");
            document(w, cx, variant);
            document_non_exhaustive(w, variant);

            use crate::clean::{Variant, VariantKind};
            if let clean::VariantItem(Variant {
                kind: VariantKind::Struct(ref s)
            }) = variant.inner {
                let variant_id = cx.derive_id(format!("{}.{}.fields",
                                                   ItemType::Variant,
                                                   variant.name.as_ref().unwrap()));
                write!(w, "<div class='autohide sub-variant' id='{id}'>",
                       id = variant_id);
                write!(w, "<h3>Fields of <b>{name}</b></h3><div>",
                       name = variant.name.as_ref().unwrap());
                for field in &s.fields {
                    use crate::clean::StructFieldItem;
                    if let StructFieldItem(ref ty) = field.inner {
                        let id = cx.derive_id(format!("variant.{}.field.{}",
                                                   variant.name.as_ref().unwrap(),
                                                   field.name.as_ref().unwrap()));
                        let ns_id = cx.derive_id(format!("{}.{}.{}.{}",
                                                      variant.name.as_ref().unwrap(),
                                                      ItemType::Variant.name_space(),
                                                      field.name.as_ref().unwrap(),
                                                      ItemType::StructField.name_space()));
                        write!(w, "<span id=\"{id}\" class=\"variant small-section-header\">\
                                   <a href=\"#{id}\" class=\"anchor field\"></a>\
                                   <code id='{ns_id}'>{f}:&nbsp;{t}\
                                   </code></span>",
                               id = id,
                               ns_id = ns_id,
                               f = field.name.as_ref().unwrap(),
                               t = ty.print());
                        document(w, cx, field);
                    }
                }
                write!(w, "</div></div>");
            }
            render_stability_since(w, variant, it);
        }
    }
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn render_attribute(attr: &ast::MetaItem) -> Option<String> {
    let path = pprust::path_to_string(&attr.path);

    if attr.is_word() {
        Some(path)
    } else if let Some(v) = attr.value_str() {
        Some(format!("{} = {:?}", path, v))
    } else if let Some(values) = attr.meta_item_list() {
        let display: Vec<_> = values.iter().filter_map(|attr| {
            attr.meta_item().and_then(|mi| render_attribute(mi))
        }).collect();

        if display.len() > 0 {
            Some(format!("{}({})", path, display.join(", ")))
        } else {
            None
        }
    } else {
        None
    }
}

const ATTRIBUTE_WHITELIST: &'static [Symbol] = &[
    sym::export_name,
    sym::lang,
    sym::link_section,
    sym::must_use,
    sym::no_mangle,
    sym::repr,
    sym::non_exhaustive
];

// The `top` parameter is used when generating the item declaration to ensure it doesn't have a
// left padding. For example:
//
// #[foo] <----- "top" attribute
// struct Foo {
//     #[bar] <---- not "top" attribute
//     bar: usize,
// }
fn render_attributes(w: &mut Buffer, it: &clean::Item, top: bool) {
    let mut attrs = String::new();

    for attr in &it.attrs.other_attrs {
        if !ATTRIBUTE_WHITELIST.contains(&attr.name_or_empty()) {
            continue;
        }
        if let Some(s) = render_attribute(&attr.meta().unwrap()) {
            attrs.push_str(&format!("#[{}]\n", s));
        }
    }
    if attrs.len() > 0 {
        write!(w, "<span class=\"docblock attributes{}\">{}</span>",
               if top { " top-attr" } else { "" }, &attrs);
    }
}

fn render_struct(w: &mut Buffer, it: &clean::Item,
                 g: Option<&clean::Generics>,
                 ty: doctree::StructType,
                 fields: &[clean::Item],
                 tab: &str,
                 structhead: bool) {
    write!(w, "{}{}{}",
           it.visibility.print_with_space(),
           if structhead {"struct "} else {""},
           it.name.as_ref().unwrap());
    if let Some(g) = g {
        write!(w, "{}", g.print())
    }
    match ty {
        doctree::Plain => {
            if let Some(g) = g {
                write!(w, "{}", WhereClause { gens: g, indent: 0, end_newline: true })
            }
            let mut has_visible_fields = false;
            write!(w, " {{");
            for field in fields {
                if let clean::StructFieldItem(ref ty) = field.inner {
                    write!(w, "\n{}    {}{}: {},",
                           tab,
                           field.visibility.print_with_space(),
                           field.name.as_ref().unwrap(),
                           ty.print());
                    has_visible_fields = true;
                }
            }

            if has_visible_fields {
                if it.has_stripped_fields().unwrap() {
                    write!(w, "\n{}    // some fields omitted", tab);
                }
                write!(w, "\n{}", tab);
            } else if it.has_stripped_fields().unwrap() {
                // If there are no visible fields we can just display
                // `{ /* fields omitted */ }` to save space.
                write!(w, " /* fields omitted */ ");
            }
            write!(w, "}}");
        }
        doctree::Tuple => {
            write!(w, "(");
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    write!(w, ", ");
                }
                match field.inner {
                    clean::StrippedItem(box clean::StructFieldItem(..)) => {
                        write!(w, "_")
                    }
                    clean::StructFieldItem(ref ty) => {
                        write!(w, "{}{}", field.visibility.print_with_space(), ty.print())
                    }
                    _ => unreachable!()
                }
            }
            write!(w, ")");
            if let Some(g) = g {
                write!(w, "{}", WhereClause { gens: g, indent: 0, end_newline: false })
            }
            write!(w, ";");
        }
        doctree::Unit => {
            // Needed for PhantomData.
            if let Some(g) = g {
                write!(w, "{}", WhereClause { gens: g, indent: 0, end_newline: false })
            }
            write!(w, ";");
        }
    }
}

fn render_union(w: &mut Buffer, it: &clean::Item,
                g: Option<&clean::Generics>,
                fields: &[clean::Item],
                tab: &str,
                structhead: bool) {
    write!(w, "{}{}{}",
           it.visibility.print_with_space(),
           if structhead {"union "} else {""},
           it.name.as_ref().unwrap());
    if let Some(g) = g {
        write!(w, "{}", g.print());
        write!(w, "{}", WhereClause { gens: g, indent: 0, end_newline: true });
    }

    write!(w, " {{\n{}", tab);
    for field in fields {
        if let clean::StructFieldItem(ref ty) = field.inner {
            write!(w, "    {}{}: {},\n{}",
                   field.visibility.print_with_space(),
                   field.name.as_ref().unwrap(),
                   ty.print(),
                   tab);
        }
    }

    if it.has_stripped_fields().unwrap() {
        write!(w, "    // some fields omitted\n{}", tab);
    }
    write!(w, "}}");
}

#[derive(Copy, Clone)]
enum AssocItemLink<'a> {
    Anchor(Option<&'a str>),
    GotoSource(DefId, &'a FxHashSet<String>),
}

impl<'a> AssocItemLink<'a> {
    fn anchor(&self, id: &'a String) -> Self {
        match *self {
            AssocItemLink::Anchor(_) => { AssocItemLink::Anchor(Some(&id)) },
            ref other => *other,
        }
    }
}

enum AssocItemRender<'a> {
    All,
    DerefFor { trait_: &'a clean::Type, type_: &'a clean::Type, deref_mut_: bool }
}

#[derive(Copy, Clone, PartialEq)]
enum RenderMode {
    Normal,
    ForDeref { mut_: bool },
}

fn render_assoc_items(w: &mut Buffer,
                      cx: &Context,
                      containing_item: &clean::Item,
                      it: DefId,
                      what: AssocItemRender<'_>) {
    let c = &cx.cache;
    let v = match c.impls.get(&it) {
        Some(v) => v,
        None => return,
    };
    let (non_trait, traits): (Vec<_>, _) = v.iter().partition(|i| {
        i.inner_impl().trait_.is_none()
    });
    if !non_trait.is_empty() {
        let render_mode = match what {
            AssocItemRender::All => {
                write!(w, "\
                    <h2 id='methods' class='small-section-header'>\
                      Methods<a href='#methods' class='anchor'></a>\
                    </h2>\
                ");
                RenderMode::Normal
            }
            AssocItemRender::DerefFor { trait_, type_, deref_mut_ } => {
                write!(w, "\
                    <h2 id='deref-methods' class='small-section-header'>\
                      Methods from {}&lt;Target = {}&gt;\
                      <a href='#deref-methods' class='anchor'></a>\
                    </h2>\
                ", trait_.print(), type_.print());
                RenderMode::ForDeref { mut_: deref_mut_ }
            }
        };
        for i in &non_trait {
            render_impl(w, cx, i, AssocItemLink::Anchor(None), render_mode,
                        containing_item.stable_since(), true, None, false, true);
        }
    }
    if let AssocItemRender::DerefFor { .. } = what {
        return;
    }
    if !traits.is_empty() {
        let deref_impl = traits.iter().find(|t| {
            t.inner_impl().trait_.def_id() == c.deref_trait_did
        });
        if let Some(impl_) = deref_impl {
            let has_deref_mut = traits.iter().find(|t| {
                t.inner_impl().trait_.def_id() == c.deref_mut_trait_did
            }).is_some();
            render_deref_methods(w, cx, impl_, containing_item, has_deref_mut);
        }

        let (synthetic, concrete): (Vec<&&Impl>, Vec<&&Impl>) = traits
            .iter()
            .partition(|t| t.inner_impl().synthetic);
        let (blanket_impl, concrete): (Vec<&&Impl>, _) = concrete
            .into_iter()
            .partition(|t| t.inner_impl().blanket_impl.is_some());

        let mut impls = Buffer::empty_from(&w);
        render_impls(cx, &mut impls, &concrete, containing_item);
        let impls = impls.into_inner();
        if !impls.is_empty() {
            write!(w, "\
                <h2 id='implementations' class='small-section-header'>\
                  Trait Implementations<a href='#implementations' class='anchor'></a>\
                </h2>\
                <div id='implementations-list'>{}</div>", impls);
        }

        if !synthetic.is_empty() {
            write!(w, "\
                <h2 id='synthetic-implementations' class='small-section-header'>\
                  Auto Trait Implementations\
                  <a href='#synthetic-implementations' class='anchor'></a>\
                </h2>\
                <div id='synthetic-implementations-list'>\
            ");
            render_impls(cx, w, &synthetic, containing_item);
            write!(w, "</div>");
        }

        if !blanket_impl.is_empty() {
            write!(w, "\
                <h2 id='blanket-implementations' class='small-section-header'>\
                  Blanket Implementations\
                  <a href='#blanket-implementations' class='anchor'></a>\
                </h2>\
                <div id='blanket-implementations-list'>\
            ");
            render_impls(cx, w, &blanket_impl, containing_item);
            write!(w, "</div>");
        }
    }
}

fn render_deref_methods(w: &mut Buffer, cx: &Context, impl_: &Impl,
                        container_item: &clean::Item, deref_mut: bool) {
    let deref_type = impl_.inner_impl().trait_.as_ref().unwrap();
    let target = impl_.inner_impl().items.iter().filter_map(|item| {
        match item.inner {
            clean::TypedefItem(ref t, true) => Some(&t.type_),
            _ => None,
        }
    }).next().expect("Expected associated type binding");
    let what = AssocItemRender::DerefFor { trait_: deref_type, type_: target,
                                           deref_mut_: deref_mut };
    if let Some(did) = target.def_id() {
        render_assoc_items(w, cx, container_item, did, what)
    } else {
        if let Some(prim) = target.primitive_type() {
            if let Some(&did) = cx.cache.primitive_locations.get(&prim) {
                render_assoc_items(w, cx, container_item, did, what);
            }
        }
    }
}

fn should_render_item(item: &clean::Item, deref_mut_: bool) -> bool {
    let self_type_opt = match item.inner {
        clean::MethodItem(ref method) => method.decl.self_type(),
        clean::TyMethodItem(ref method) => method.decl.self_type(),
        _ => None
    };

    if let Some(self_ty) = self_type_opt {
        let (by_mut_ref, by_box, by_value) = match self_ty {
            SelfTy::SelfBorrowed(_, mutability) |
            SelfTy::SelfExplicit(clean::BorrowedRef { mutability, .. }) => {
                (mutability == Mutability::Mutable, false, false)
            },
            SelfTy::SelfExplicit(clean::ResolvedPath { did, .. }) => {
                (false, Some(did) == cache().owned_box_did, false)
            },
            SelfTy::SelfValue => (false, false, true),
            _ => (false, false, false),
        };

        (deref_mut_ || !by_mut_ref) && !by_box && !by_value
    } else {
        false
    }
}

fn render_spotlight_traits(item: &clean::Item) -> String {
    match item.inner {
        clean::FunctionItem(clean::Function { ref decl, .. }) |
        clean::TyMethodItem(clean::TyMethod { ref decl, .. }) |
        clean::MethodItem(clean::Method { ref decl, .. }) |
        clean::ForeignFunctionItem(clean::Function { ref decl, .. }) => {
            spotlight_decl(decl)
        }
        _ => String::new()
    }
}

fn spotlight_decl(decl: &clean::FnDecl) -> String {
    let mut out = Buffer::html();
    let mut trait_ = String::new();

    if let Some(did) = decl.output.def_id() {
        let c = cache();
        if let Some(impls) = c.impls.get(&did) {
            for i in impls {
                let impl_ = i.inner_impl();
                if impl_.trait_.def_id().map_or(false, |d| c.traits[&d].is_spotlight) {
                    if out.is_empty() {
                        out.push_str(
                            &format!("<h3 class=\"important\">Important traits for {}</h3>\
                                      <code class=\"content\">",
                                     impl_.for_.print()));
                        trait_.push_str(&impl_.for_.print().to_string());
                    }

                    //use the "where" class here to make it small
                    out.push_str(
                        &format!("<span class=\"where fmt-newline\">{}</span>", impl_.print()));
                    let t_did = impl_.trait_.def_id().unwrap();
                    for it in &impl_.items {
                        if let clean::TypedefItem(ref tydef, _) = it.inner {
                            out.push_str("<span class=\"where fmt-newline\">    ");
                            assoc_type(&mut out, it, &[],
                                       Some(&tydef.type_),
                                       AssocItemLink::GotoSource(t_did, &FxHashSet::default()),
                                       "");
                            out.push_str(";</span>");
                        }
                    }
                }
            }
        }
    }

    if !out.is_empty() {
        out.insert_str(0, &format!("<div class=\"important-traits\"><div class='tooltip'>ⓘ\
                                    <span class='tooltiptext'>Important traits for {}</span></div>\
                                    <div class=\"content hidden\">",
                                   trait_));
        out.push_str("</code></div></div>");
    }

    out.into_inner()
}

fn render_impl(w: &mut Buffer, cx: &Context, i: &Impl, link: AssocItemLink<'_>,
               render_mode: RenderMode, outer_version: Option<&str>, show_def_docs: bool,
               use_absolute: Option<bool>, is_on_foreign_type: bool,
               show_default_items: bool) {
    if render_mode == RenderMode::Normal {
        let id = cx.derive_id(match i.inner_impl().trait_ {
            Some(ref t) => if is_on_foreign_type {
                get_id_for_impl_on_foreign_type(&i.inner_impl().for_, t)
            } else {
                format!("impl-{}", small_url_encode(&format!("{:#}", t.print())))
            },
            None => "impl".to_string(),
        });
        if let Some(use_absolute) = use_absolute {
            write!(w, "<h3 id='{}' class='impl'><code class='in-band'>", id);
            fmt_impl_for_trait_page(&i.inner_impl(), w, use_absolute);
            if show_def_docs {
                for it in &i.inner_impl().items {
                    if let clean::TypedefItem(ref tydef, _) = it.inner {
                        write!(w, "<span class=\"where fmt-newline\">  ");
                        assoc_type(w, it, &vec![], Some(&tydef.type_),
                                   AssocItemLink::Anchor(None),
                                   "");
                        write!(w, ";</span>");
                    }
                }
            }
            write!(w, "</code>");
        } else {
            write!(w, "<h3 id='{}' class='impl'><code class='in-band'>{}</code>",
                id, i.inner_impl().print()
            );
        }
        write!(w, "<a href='#{}' class='anchor'></a>", id);
        let since = i.impl_item.stability.as_ref().map(|s| &s.since[..]);
        render_stability_since_raw(w, since, outer_version);
        if let Some(l) = cx.src_href(&i.impl_item) {
            write!(w, "<a class='srclink' href='{}' title='{}'>[src]</a>",
                   l, "goto source code");
        }
        write!(w, "</h3>");
        if let Some(ref dox) = cx.shared.maybe_collapsed_doc_value(&i.impl_item) {
            let mut ids = cx.id_map.borrow_mut();
            write!(w, "<div class='docblock'>{}</div>",
                   Markdown(&*dox, &i.impl_item.links(), &mut ids,
                            cx.shared.codes, cx.shared.edition, &cx.shared.playground).to_string());
        }
    }

    fn doc_impl_item(w: &mut Buffer, cx: &Context, item: &clean::Item,
                     link: AssocItemLink<'_>, render_mode: RenderMode,
                     is_default_item: bool, outer_version: Option<&str>,
                     trait_: Option<&clean::Trait>, show_def_docs: bool) {
        let item_type = item.type_();
        let name = item.name.as_ref().unwrap();

        let render_method_item = match render_mode {
            RenderMode::Normal => true,
            RenderMode::ForDeref { mut_: deref_mut_ } => should_render_item(&item, deref_mut_),
        };

        let (is_hidden, extra_class) = if (trait_.is_none() ||
                                           item.doc_value().is_some() ||
                                           item.inner.is_associated()) &&
                                          !is_default_item {
            (false, "")
        } else {
            (true, " hidden")
        };
        match item.inner {
            clean::MethodItem(clean::Method { ref decl, .. }) |
            clean::TyMethodItem(clean::TyMethod { ref decl, .. }) => {
                // Only render when the method is not static or we allow static methods
                if render_method_item {
                    let id = cx.derive_id(format!("{}.{}", item_type, name));
                    let ns_id = cx.derive_id(format!("{}.{}",
                            name, item_type.name_space()));
                    write!(w, "<h4 id='{}' class=\"{}{}\">",
                        id, item_type, extra_class);
                    write!(w, "{}", spotlight_decl(decl));
                    write!(w, "<code id='{}'>", ns_id);
                    render_assoc_item(w, item, link.anchor(&id), ItemType::Impl);
                    write!(w, "</code>");
                    render_stability_since_raw(w, item.stable_since(), outer_version);
                    if let Some(l) = cx.src_href(item) {
                        write!(w, "<a class='srclink' href='{}' title='{}'>[src]</a>",
                               l, "goto source code");
                    }
                    write!(w, "</h4>");
                }
            }
            clean::TypedefItem(ref tydef, _) => {
                let id = cx.derive_id(format!("{}.{}", ItemType::AssocType, name));
                let ns_id = cx.derive_id(format!("{}.{}", name, item_type.name_space()));
                write!(w, "<h4 id='{}' class=\"{}{}\">", id, item_type, extra_class);
                write!(w, "<code id='{}'>", ns_id);
                assoc_type(w, item, &Vec::new(), Some(&tydef.type_), link.anchor(&id), "");
                write!(w, "</code></h4>");
            }
            clean::AssocConstItem(ref ty, ref default) => {
                let id = cx.derive_id(format!("{}.{}", item_type, name));
                let ns_id = cx.derive_id(format!("{}.{}", name, item_type.name_space()));
                write!(w, "<h4 id='{}' class=\"{}{}\">", id, item_type, extra_class);
                write!(w, "<code id='{}'>", ns_id);
                assoc_const(w, item, ty, default.as_ref(), link.anchor(&id), "");
                write!(w, "</code>");
                render_stability_since_raw(w, item.stable_since(), outer_version);
                if let Some(l) = cx.src_href(item) {
                    write!(w, "<a class='srclink' href='{}' title='{}'>[src]</a>",
                            l, "goto source code");
                }
                write!(w, "</h4>");
            }
            clean::AssocTypeItem(ref bounds, ref default) => {
                let id = cx.derive_id(format!("{}.{}", item_type, name));
                let ns_id = cx.derive_id(format!("{}.{}", name, item_type.name_space()));
                write!(w, "<h4 id='{}' class=\"{}{}\">", id, item_type, extra_class);
                write!(w, "<code id='{}'>", ns_id);
                assoc_type(w, item, bounds, default.as_ref(), link.anchor(&id), "");
                write!(w, "</code></h4>");
            }
            clean::StrippedItem(..) => return,
            _ => panic!("can't make docs for trait item with name {:?}", item.name)
        }

        if render_method_item {
            if !is_default_item {
                if let Some(t) = trait_ {
                    // The trait item may have been stripped so we might not
                    // find any documentation or stability for it.
                    if let Some(it) = t.items.iter().find(|i| i.name == item.name) {
                        // We need the stability of the item from the trait
                        // because impls can't have a stability.
                        document_stability(w, cx, it, is_hidden);
                        if item.doc_value().is_some() {
                            document_full(w, item, cx, "", is_hidden);
                        } else if show_def_docs {
                            // In case the item isn't documented,
                            // provide short documentation from the trait.
                            document_short(w, cx, it, link, "", is_hidden);
                        }
                    }
                } else {
                    document_stability(w, cx, item, is_hidden);
                    if show_def_docs {
                        document_full(w, item, cx, "", is_hidden);
                    }
                }
            } else {
                document_stability(w, cx, item, is_hidden);
                if show_def_docs {
                    document_short(w, cx, item, link, "", is_hidden);
                }
            }
        }
    }

    let traits = &cx.cache.traits;
    let trait_ = i.trait_did().map(|did| &traits[&did]);

    write!(w, "<div class='impl-items'>");
    for trait_item in &i.inner_impl().items {
        doc_impl_item(w, cx, trait_item, link, render_mode,
                      false, outer_version, trait_, show_def_docs);
    }

    fn render_default_items(w: &mut Buffer,
                            cx: &Context,
                            t: &clean::Trait,
                            i: &clean::Impl,
                            render_mode: RenderMode,
                            outer_version: Option<&str>,
                            show_def_docs: bool) {
        for trait_item in &t.items {
            let n = trait_item.name.clone();
            if i.items.iter().find(|m| m.name == n).is_some() {
                continue;
            }
            let did = i.trait_.as_ref().unwrap().def_id().unwrap();
            let assoc_link = AssocItemLink::GotoSource(did, &i.provided_trait_methods);

            doc_impl_item(w, cx, trait_item, assoc_link, render_mode, true,
                          outer_version, None, show_def_docs);
        }
    }

    // If we've implemented a trait, then also emit documentation for all
    // default items which weren't overridden in the implementation block.
    // We don't emit documentation for default items if they appear in the
    // Implementations on Foreign Types or Implementors sections.
    if show_default_items {
        if let Some(t) = trait_ {
            render_default_items(w, cx, t, &i.inner_impl(),
                                render_mode, outer_version, show_def_docs);
        }
    }
    write!(w, "</div>");
}

fn item_opaque_ty(
    w: &mut Buffer,
    cx: &Context,
    it: &clean::Item,
    t: &clean::OpaqueTy,
) {
    write!(w, "<pre class='rust opaque'>");
    render_attributes(w, it, false);
    write!(w, "type {}{}{where_clause} = impl {bounds};</pre>",
           it.name.as_ref().unwrap(),
           t.generics.print(),
           where_clause = WhereClause { gens: &t.generics, indent: 0, end_newline: true },
           bounds = bounds(&t.bounds, false));

    document(w, cx, it);

    // Render any items associated directly to this alias, as otherwise they
    // won't be visible anywhere in the docs. It would be nice to also show
    // associated items from the aliased type (see discussion in #32077), but
    // we need #14072 to make sense of the generics.
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_trait_alias(w: &mut Buffer, cx: &Context, it: &clean::Item,
                    t: &clean::TraitAlias) {
    write!(w, "<pre class='rust trait-alias'>");
    render_attributes(w, it, false);
    write!(w, "trait {}{}{} = {};</pre>",
           it.name.as_ref().unwrap(),
           t.generics.print(),
           WhereClause { gens: &t.generics, indent: 0, end_newline: true },
           bounds(&t.bounds, true));

    document(w, cx, it);

    // Render any items associated directly to this alias, as otherwise they
    // won't be visible anywhere in the docs. It would be nice to also show
    // associated items from the aliased type (see discussion in #32077), but
    // we need #14072 to make sense of the generics.
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_typedef(w: &mut Buffer, cx: &Context, it: &clean::Item, t: &clean::Typedef) {
    write!(w, "<pre class='rust typedef'>");
    render_attributes(w, it, false);
    write!(w, "type {}{}{where_clause} = {type_};</pre>",
           it.name.as_ref().unwrap(),
           t.generics.print(),
           where_clause = WhereClause { gens: &t.generics, indent: 0, end_newline: true },
           type_ = t.type_.print());

    document(w, cx, it);

    // Render any items associated directly to this alias, as otherwise they
    // won't be visible anywhere in the docs. It would be nice to also show
    // associated items from the aliased type (see discussion in #32077), but
    // we need #14072 to make sense of the generics.
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_foreign_type(w: &mut Buffer, cx: &Context, it: &clean::Item) {
    writeln!(w, "<pre class='rust foreigntype'>extern {{");
    render_attributes(w, it, false);
    write!(
        w,
        "    {}type {};\n}}</pre>",
        it.visibility.print_with_space(),
        it.name.as_ref().unwrap(),
    );

    document(w, cx, it);

    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn print_sidebar(cx: &Context, it: &clean::Item, buffer: &mut Buffer) {
    let parentlen = cx.current.len() - if it.is_mod() {1} else {0};

    if it.is_struct() || it.is_trait() || it.is_primitive() || it.is_union()
        || it.is_enum() || it.is_mod() || it.is_typedef() {
        write!(buffer, "<p class='location'>{}{}</p>",
            match it.inner {
                clean::StructItem(..) => "Struct ",
                clean::TraitItem(..) => "Trait ",
                clean::PrimitiveItem(..) => "Primitive Type ",
                clean::UnionItem(..) => "Union ",
                clean::EnumItem(..) => "Enum ",
                clean::TypedefItem(..) => "Type Definition ",
                clean::ForeignTypeItem => "Foreign Type ",
                clean::ModuleItem(..) => if it.is_crate() {
                    "Crate "
                } else {
                    "Module "
                },
                _ => "",
            },
            it.name.as_ref().unwrap());
    }

    if it.is_crate() {
        if let Some(ref version) = cx.cache.crate_version {
            write!(buffer,
                    "<div class='block version'>\
                    <p>Version {}</p>\
                    </div>",
                    version);
        }
    }

    write!(buffer, "<div class=\"sidebar-elems\">");
    if it.is_crate() {
        write!(buffer, "<a id='all-types' href='all.html'><p>See all {}'s items</p></a>",
                it.name.as_ref().expect("crates always have a name"));
    }
    match it.inner {
        clean::StructItem(ref s) => sidebar_struct(buffer, it, s),
        clean::TraitItem(ref t) => sidebar_trait(buffer, it, t),
        clean::PrimitiveItem(_) => sidebar_primitive(buffer, it),
        clean::UnionItem(ref u) => sidebar_union(buffer, it, u),
        clean::EnumItem(ref e) => sidebar_enum(buffer, it, e),
        clean::TypedefItem(_, _) => sidebar_typedef(buffer, it),
        clean::ModuleItem(ref m) => sidebar_module(buffer, &m.items),
        clean::ForeignTypeItem => sidebar_foreign_type(buffer, it),
        _ => (),
    }

    // The sidebar is designed to display sibling functions, modules and
    // other miscellaneous information. since there are lots of sibling
    // items (and that causes quadratic growth in large modules),
    // we refactor common parts into a shared JavaScript file per module.
    // still, we don't move everything into JS because we want to preserve
    // as much HTML as possible in order to allow non-JS-enabled browsers
    // to navigate the documentation (though slightly inefficiently).

    write!(buffer, "<p class='location'>");
    for (i, name) in cx.current.iter().take(parentlen).enumerate() {
        if i > 0 {
            write!(buffer, "::<wbr>");
        }
        write!(buffer, "<a href='{}index.html'>{}</a>",
                &cx.root_path()[..(cx.current.len() - i - 1) * 3],
                *name);
    }
    write!(buffer, "</p>");

    // Sidebar refers to the enclosing module, not this module.
    let relpath = if it.is_mod() { "../" } else { "" };
    write!(buffer,
            "<script>window.sidebarCurrent = {{\
                name: '{name}', \
                ty: '{ty}', \
                relpath: '{path}'\
            }};</script>",
            name = it.name.as_ref().map(|x| &x[..]).unwrap_or(""),
            ty = it.type_(),
            path = relpath);
    if parentlen == 0 {
        // There is no sidebar-items.js beyond the crate root path
        // FIXME maybe dynamic crate loading can be merged here
    } else {
        write!(buffer, "<script defer src=\"{path}sidebar-items.js\"></script>",
                path = relpath);
    }
    // Closes sidebar-elems div.
    write!(buffer, "</div>");
}

fn get_next_url(used_links: &mut FxHashSet<String>, url: String) -> String {
    if used_links.insert(url.clone()) {
        return url;
    }
    let mut add = 1;
    while used_links.insert(format!("{}-{}", url, add)) == false {
        add += 1;
    }
    format!("{}-{}", url, add)
}

fn get_methods(
    i: &clean::Impl,
    for_deref: bool,
    used_links: &mut FxHashSet<String>,
    deref_mut: bool,
) -> Vec<String> {
    i.items.iter().filter_map(|item| {
        match item.name {
            Some(ref name) if !name.is_empty() && item.is_method() => {
                if !for_deref || should_render_item(item, deref_mut) {
                    Some(format!("<a href=\"#{}\">{}</a>",
                                 get_next_url(used_links, format!("method.{}", name)),
                                 name))
                } else {
                    None
                }
            }
            _ => None,
        }
    }).collect::<Vec<_>>()
}

// The point is to url encode any potential character from a type with genericity.
fn small_url_encode(s: &str) -> String {
    s.replace("<", "%3C")
     .replace(">", "%3E")
     .replace(" ", "%20")
     .replace("?", "%3F")
     .replace("'", "%27")
     .replace("&", "%26")
     .replace(",", "%2C")
     .replace(":", "%3A")
     .replace(";", "%3B")
     .replace("[", "%5B")
     .replace("]", "%5D")
     .replace("\"", "%22")
}

fn sidebar_assoc_items(it: &clean::Item) -> String {
    let mut out = String::new();
    let c = cache();
    if let Some(v) = c.impls.get(&it.def_id) {
        let mut used_links = FxHashSet::default();

        {
            let used_links_bor = &mut used_links;
            let mut ret = v.iter()
                           .filter(|i| i.inner_impl().trait_.is_none())
                           .flat_map(move |i| get_methods(i.inner_impl(),
                                                          false,
                                                          used_links_bor, false))
                           .collect::<Vec<_>>();
            // We want links' order to be reproducible so we don't use unstable sort.
            ret.sort();
            if !ret.is_empty() {
                out.push_str(&format!("<a class=\"sidebar-title\" href=\"#methods\">Methods\
                                       </a><div class=\"sidebar-links\">{}</div>", ret.join("")));
            }
        }

        if v.iter().any(|i| i.inner_impl().trait_.is_some()) {
            if let Some(impl_) = v.iter()
                                  .filter(|i| i.inner_impl().trait_.is_some())
                                  .find(|i| i.inner_impl().trait_.def_id() == c.deref_trait_did) {
                if let Some(target) = impl_.inner_impl().items.iter().filter_map(|item| {
                    match item.inner {
                        clean::TypedefItem(ref t, true) => Some(&t.type_),
                        _ => None,
                    }
                }).next() {
                    let inner_impl = target.def_id().or(target.primitive_type().and_then(|prim| {
                        c.primitive_locations.get(&prim).cloned()
                    })).and_then(|did| c.impls.get(&did));
                    if let Some(impls) = inner_impl {
                        out.push_str("<a class=\"sidebar-title\" href=\"#deref-methods\">");
                        out.push_str(&format!("Methods from {}&lt;Target={}&gt;",
                            Escape(&format!(
                                "{:#}", impl_.inner_impl().trait_.as_ref().unwrap().print()
                            )),
                            Escape(&format!("{:#}", target.print()))));
                        out.push_str("</a>");
                        let mut ret = impls.iter()
                                           .filter(|i| i.inner_impl().trait_.is_none())
                                           .flat_map(|i| get_methods(i.inner_impl(),
                                                                     true,
                                                                     &mut used_links,
                                                                     true))
                                           .collect::<Vec<_>>();
                        // We want links' order to be reproducible so we don't use unstable sort.
                        ret.sort();
                        if !ret.is_empty() {
                            out.push_str(&format!("<div class=\"sidebar-links\">{}</div>",
                                                  ret.join("")));
                        }
                    }
                }
            }
            let format_impls = |impls: Vec<&Impl>| {
                let mut links = FxHashSet::default();

                let mut ret = impls.iter()
                    .filter_map(|i| {
                        let is_negative_impl = is_negative_impl(i.inner_impl());
                        if let Some(ref i) = i.inner_impl().trait_ {
                            let i_display = format!("{:#}", i.print());
                            let out = Escape(&i_display);
                            let encoded = small_url_encode(&format!("{:#}", i.print()));
                            let generated = format!("<a href=\"#impl-{}\">{}{}</a>",
                                                    encoded,
                                                    if is_negative_impl { "!" } else { "" },
                                                    out);
                            if links.insert(generated.clone()) {
                                Some(generated)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<String>>();
                ret.sort();
                ret.join("")
            };

            let (synthetic, concrete): (Vec<&Impl>, Vec<&Impl>) = v
                .iter()
                .partition::<Vec<_>, _>(|i| i.inner_impl().synthetic);
            let (blanket_impl, concrete): (Vec<&Impl>, Vec<&Impl>) = concrete
                .into_iter()
                .partition::<Vec<_>, _>(|i| i.inner_impl().blanket_impl.is_some());

            let concrete_format = format_impls(concrete);
            let synthetic_format = format_impls(synthetic);
            let blanket_format = format_impls(blanket_impl);

            if !concrete_format.is_empty() {
                out.push_str("<a class=\"sidebar-title\" href=\"#implementations\">\
                              Trait Implementations</a>");
                out.push_str(&format!("<div class=\"sidebar-links\">{}</div>", concrete_format));
            }

            if !synthetic_format.is_empty() {
                out.push_str("<a class=\"sidebar-title\" href=\"#synthetic-implementations\">\
                              Auto Trait Implementations</a>");
                out.push_str(&format!("<div class=\"sidebar-links\">{}</div>", synthetic_format));
            }

            if !blanket_format.is_empty() {
                out.push_str("<a class=\"sidebar-title\" href=\"#blanket-implementations\">\
                              Blanket Implementations</a>");
                out.push_str(&format!("<div class=\"sidebar-links\">{}</div>", blanket_format));
            }
        }
    }

    out
}

fn sidebar_struct(buf: &mut Buffer, it: &clean::Item, s: &clean::Struct) {
    let mut sidebar = String::new();
    let fields = get_struct_fields_name(&s.fields);

    if !fields.is_empty() {
        if let doctree::Plain = s.struct_type {
            sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#fields\">Fields</a>\
                                       <div class=\"sidebar-links\">{}</div>", fields));
        }
    }

    sidebar.push_str(&sidebar_assoc_items(it));

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn get_id_for_impl_on_foreign_type(for_: &clean::Type, trait_: &clean::Type) -> String {
    small_url_encode(&format!("impl-{:#}-for-{:#}", trait_.print(), for_.print()))
}

fn extract_for_impl_name(item: &clean::Item) -> Option<(String, String)> {
    match item.inner {
        clean::ItemEnum::ImplItem(ref i) => {
            if let Some(ref trait_) = i.trait_ {
                Some((
                    format!("{:#}", i.for_.print()),
                    get_id_for_impl_on_foreign_type(&i.for_, trait_),
                ))
            } else {
                None
            }
        },
        _ => None,
    }
}

fn is_negative_impl(i: &clean::Impl) -> bool {
    i.polarity == Some(clean::ImplPolarity::Negative)
}

fn sidebar_trait(buf: &mut Buffer, it: &clean::Item, t: &clean::Trait) {
    let mut sidebar = String::new();

    let types = t.items
                 .iter()
                 .filter_map(|m| {
                     match m.name {
                         Some(ref name) if m.is_associated_type() => {
                             Some(format!("<a href=\"#associatedtype.{name}\">{name}</a>",
                                          name=name))
                         }
                         _ => None,
                     }
                 })
                 .collect::<String>();
    let consts = t.items
                  .iter()
                  .filter_map(|m| {
                      match m.name {
                          Some(ref name) if m.is_associated_const() => {
                              Some(format!("<a href=\"#associatedconstant.{name}\">{name}</a>",
                                           name=name))
                          }
                          _ => None,
                      }
                  })
                  .collect::<String>();
    let mut required = t.items
                        .iter()
                        .filter_map(|m| {
                            match m.name {
                                Some(ref name) if m.is_ty_method() => {
                                    Some(format!("<a href=\"#tymethod.{name}\">{name}</a>",
                                                 name=name))
                                }
                                _ => None,
                            }
                        })
                        .collect::<Vec<String>>();
    let mut provided = t.items
                        .iter()
                        .filter_map(|m| {
                            match m.name {
                                Some(ref name) if m.is_method() => {
                                    Some(format!("<a href=\"#method.{0}\">{0}</a>", name))
                                }
                                _ => None,
                            }
                        })
                        .collect::<Vec<String>>();

    if !types.is_empty() {
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#associated-types\">\
                                   Associated Types</a><div class=\"sidebar-links\">{}</div>",
                                  types));
    }
    if !consts.is_empty() {
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#associated-const\">\
                                   Associated Constants</a><div class=\"sidebar-links\">{}</div>",
                                  consts));
    }
    if !required.is_empty() {
        required.sort();
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#required-methods\">\
                                   Required Methods</a><div class=\"sidebar-links\">{}</div>",
                                  required.join("")));
    }
    if !provided.is_empty() {
        provided.sort();
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#provided-methods\">\
                                   Provided Methods</a><div class=\"sidebar-links\">{}</div>",
                                  provided.join("")));
    }

    let c = cache();

    if let Some(implementors) = c.implementors.get(&it.def_id) {
        let mut res = implementors.iter()
                                  .filter(|i| i.inner_impl().for_.def_id()
                                  .map_or(false, |d| !c.paths.contains_key(&d)))
                                  .filter_map(|i| {
                                      match extract_for_impl_name(&i.impl_item) {
                                          Some((ref name, ref id)) => {
                                              Some(format!("<a href=\"#{}\">{}</a>",
                                                          id,
                                                          Escape(name)))
                                          }
                                          _ => None,
                                      }
                                  })
                                  .collect::<Vec<String>>();
        if !res.is_empty() {
            res.sort();
            sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#foreign-impls\">\
                                       Implementations on Foreign Types</a><div \
                                       class=\"sidebar-links\">{}</div>",
                                      res.join("")));
        }
    }

    sidebar.push_str("<a class=\"sidebar-title\" href=\"#implementors\">Implementors</a>");
    if t.auto {
        sidebar.push_str("<a class=\"sidebar-title\" \
                          href=\"#synthetic-implementors\">Auto Implementors</a>");
    }

    sidebar.push_str(&sidebar_assoc_items(it));

    write!(buf, "<div class=\"block items\">{}</div>", sidebar)
}

fn sidebar_primitive(buf: &mut Buffer, it: &clean::Item) {
    let sidebar = sidebar_assoc_items(it);

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn sidebar_typedef(buf: &mut Buffer, it: &clean::Item) {
    let sidebar = sidebar_assoc_items(it);

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn get_struct_fields_name(fields: &[clean::Item]) -> String {
    fields.iter()
          .filter(|f| if let clean::StructFieldItem(..) = f.inner {
              true
          } else {
              false
          })
          .filter_map(|f| match f.name {
              Some(ref name) => Some(format!("<a href=\"#structfield.{name}\">\
                                              {name}</a>", name=name)),
              _ => None,
          })
          .collect()
}

fn sidebar_union(buf: &mut Buffer, it: &clean::Item, u: &clean::Union) {
    let mut sidebar = String::new();
    let fields = get_struct_fields_name(&u.fields);

    if !fields.is_empty() {
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#fields\">Fields</a>\
                                   <div class=\"sidebar-links\">{}</div>", fields));
    }

    sidebar.push_str(&sidebar_assoc_items(it));

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn sidebar_enum(buf: &mut Buffer, it: &clean::Item, e: &clean::Enum) {
    let mut sidebar = String::new();

    let variants = e.variants.iter()
                             .filter_map(|v| match v.name {
                                 Some(ref name) => Some(format!("<a href=\"#variant.{name}\">{name}\
                                                                 </a>", name = name)),
                                 _ => None,
                             })
                             .collect::<String>();
    if !variants.is_empty() {
        sidebar.push_str(&format!("<a class=\"sidebar-title\" href=\"#variants\">Variants</a>\
                                   <div class=\"sidebar-links\">{}</div>", variants));
    }

    sidebar.push_str(&sidebar_assoc_items(it));

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn item_ty_to_strs(ty: &ItemType) -> (&'static str, &'static str) {
    match *ty {
        ItemType::ExternCrate |
        ItemType::Import          => ("reexports", "Re-exports"),
        ItemType::Module          => ("modules", "Modules"),
        ItemType::Struct          => ("structs", "Structs"),
        ItemType::Union           => ("unions", "Unions"),
        ItemType::Enum            => ("enums", "Enums"),
        ItemType::Function        => ("functions", "Functions"),
        ItemType::Typedef         => ("types", "Type Definitions"),
        ItemType::Static          => ("statics", "Statics"),
        ItemType::Constant        => ("constants", "Constants"),
        ItemType::Trait           => ("traits", "Traits"),
        ItemType::Impl            => ("impls", "Implementations"),
        ItemType::TyMethod        => ("tymethods", "Type Methods"),
        ItemType::Method          => ("methods", "Methods"),
        ItemType::StructField     => ("fields", "Struct Fields"),
        ItemType::Variant         => ("variants", "Variants"),
        ItemType::Macro           => ("macros", "Macros"),
        ItemType::Primitive       => ("primitives", "Primitive Types"),
        ItemType::AssocType       => ("associated-types", "Associated Types"),
        ItemType::AssocConst      => ("associated-consts", "Associated Constants"),
        ItemType::ForeignType     => ("foreign-types", "Foreign Types"),
        ItemType::Keyword         => ("keywords", "Keywords"),
        ItemType::OpaqueTy        => ("opaque-types", "Opaque Types"),
        ItemType::ProcAttribute   => ("attributes", "Attribute Macros"),
        ItemType::ProcDerive      => ("derives", "Derive Macros"),
        ItemType::TraitAlias      => ("trait-aliases", "Trait aliases"),
    }
}

fn sidebar_module(buf: &mut Buffer, items: &[clean::Item]) {
    let mut sidebar = String::new();

    if items.iter().any(|it| it.type_() == ItemType::ExternCrate ||
                             it.type_() == ItemType::Import) {
        sidebar.push_str(&format!("<li><a href=\"#{id}\">{name}</a></li>",
                                  id = "reexports",
                                  name = "Re-exports"));
    }

    // ordering taken from item_module, reorder, where it prioritized elements in a certain order
    // to print its headings
    for &myty in &[ItemType::Primitive, ItemType::Module, ItemType::Macro, ItemType::Struct,
                   ItemType::Enum, ItemType::Constant, ItemType::Static, ItemType::Trait,
                   ItemType::Function, ItemType::Typedef, ItemType::Union, ItemType::Impl,
                   ItemType::TyMethod, ItemType::Method, ItemType::StructField, ItemType::Variant,
                   ItemType::AssocType, ItemType::AssocConst, ItemType::ForeignType,
                   ItemType::Keyword] {
        if items.iter().any(|it| !it.is_stripped() && it.type_() == myty) {
            let (short, name) = item_ty_to_strs(&myty);
            sidebar.push_str(&format!("<li><a href=\"#{id}\">{name}</a></li>",
                                      id = short,
                                      name = name));
        }
    }

    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\"><ul>{}</ul></div>", sidebar);
    }
}

fn sidebar_foreign_type(buf: &mut Buffer, it: &clean::Item) {
    let sidebar = sidebar_assoc_items(it);
    if !sidebar.is_empty() {
        write!(buf, "<div class=\"block items\">{}</div>", sidebar);
    }
}

fn item_macro(w: &mut Buffer, cx: &Context, it: &clean::Item, t: &clean::Macro) {
    wrap_into_docblock(w, |w| {
        w.write_str(&highlight::render_with_highlighting(&t.source,
                                                         Some("macro"),
                                                         None,
                                                         None))
    });
    document(w, cx, it)
}

fn item_proc_macro(w: &mut Buffer, cx: &Context, it: &clean::Item, m: &clean::ProcMacro) {
    let name = it.name.as_ref().expect("proc-macros always have names");
    match m.kind {
        MacroKind::Bang => {
            write!(w, "<pre class='rust macro'>");
            write!(w, "{}!() {{ /* proc-macro */ }}", name);
            write!(w, "</pre>");
        }
        MacroKind::Attr => {
            write!(w, "<pre class='rust attr'>");
            write!(w, "#[{}]", name);
            write!(w, "</pre>");
        }
        MacroKind::Derive => {
            write!(w, "<pre class='rust derive'>");
            write!(w, "#[derive({})]", name);
            if !m.helpers.is_empty() {
                writeln!(w, "\n{{");
                writeln!(w, "    // Attributes available to this derive:");
                for attr in &m.helpers {
                    writeln!(w, "    #[{}]", attr);
                }
                write!(w, "}}");
            }
            write!(w, "</pre>");
        }
    }
    document(w, cx, it)
}

fn item_primitive(w: &mut Buffer, cx: &Context, it: &clean::Item) {
    document(w, cx, it);
    render_assoc_items(w, cx, it, it.def_id, AssocItemRender::All)
}

fn item_keyword(w: &mut Buffer, cx: &Context, it: &clean::Item) {
    document(w, cx, it)
}

crate const BASIC_KEYWORDS: &'static str = "rust, rustlang, rust-lang";

fn make_item_keywords(it: &clean::Item) -> String {
    format!("{}, {}", BASIC_KEYWORDS, it.name.as_ref().unwrap())
}

/// Returns a list of all paths used in the type.
/// This is used to help deduplicate imported impls
/// for reexported types. If any of the contained
/// types are re-exported, we don't use the corresponding
/// entry from the js file, as inlining will have already
/// picked up the impl
fn collect_paths_for_type(first_ty: clean::Type) -> Vec<String> {
    let mut out = Vec::new();
    let mut visited = FxHashSet::default();
    let mut work = VecDeque::new();
    let cache = cache();

    work.push_back(first_ty);

    while let Some(ty) = work.pop_front() {
        if !visited.insert(ty.clone()) {
            continue;
        }

        match ty {
            clean::Type::ResolvedPath { did, .. } => {
                let get_extern = || cache.external_paths.get(&did).map(|s| s.0.clone());
                let fqp = cache.exact_paths.get(&did).cloned().or_else(get_extern);

                match fqp {
                    Some(path) => {
                        out.push(path.join("::"));
                    },
                    _ => {}
                };

            },
            clean::Type::Tuple(tys) => {
                work.extend(tys.into_iter());
            },
            clean::Type::Slice(ty) => {
                work.push_back(*ty);
            }
            clean::Type::Array(ty, _) => {
                work.push_back(*ty);
            },
            clean::Type::RawPointer(_, ty) => {
                work.push_back(*ty);
            },
            clean::Type::BorrowedRef { type_, .. } => {
                work.push_back(*type_);
            },
            clean::Type::QPath { self_type, trait_, .. } => {
                work.push_back(*self_type);
                work.push_back(*trait_);
            },
            _ => {}
        }
    };
    out
}

crate fn cache() -> Arc<Cache> {
    CACHE_KEY.with(|c| c.borrow().clone())
}
