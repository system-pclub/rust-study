use crate::clean;
use crate::docfs::PathError;
use crate::fold::DocFolder;
use crate::html::layout;
use crate::html::render::{Error, SharedContext, BASIC_KEYWORDS};
use crate::html::highlight;
use crate::html::format::Buffer;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use syntax::source_map::FileName;

crate fn render(dst: &Path, scx: &mut SharedContext,
                  krate: clean::Crate) -> Result<clean::Crate, Error> {
    info!("emitting source files");
    let dst = dst.join("src").join(&krate.name);
    scx.ensure_dir(&dst)?;
    let mut folder = SourceCollector {
        dst,
        scx,
    };
    Ok(folder.fold_crate(krate))
}

/// Helper struct to render all source code to HTML pages
struct SourceCollector<'a> {
    scx: &'a mut SharedContext,

    /// Root destination to place all HTML output into
    dst: PathBuf,
}

impl<'a> DocFolder for SourceCollector<'a> {
    fn fold_item(&mut self, item: clean::Item) -> Option<clean::Item> {
        // If we're including source files, and we haven't seen this file yet,
        // then we need to render it out to the filesystem.
        if self.scx.include_sources
            // skip all invalid or macro spans
            && item.source.filename.is_real()
            // skip non-local items
            && item.def_id.is_local() {

            // If it turns out that we couldn't read this file, then we probably
            // can't read any of the files (generating html output from json or
            // something like that), so just don't include sources for the
            // entire crate. The other option is maintaining this mapping on a
            // per-file basis, but that's probably not worth it...
            self.scx
                .include_sources = match self.emit_source(&item.source.filename) {
                Ok(()) => true,
                Err(e) => {
                    println!("warning: source code was requested to be rendered, \
                              but processing `{}` had an error: {}",
                             item.source.filename, e);
                    println!("         skipping rendering of source code");
                    false
                }
            };
        }
        self.fold_item_recur(item)
    }
}

impl<'a> SourceCollector<'a> {
    /// Renders the given filename into its corresponding HTML source file.
    fn emit_source(&mut self, filename: &FileName) -> Result<(), Error> {
        let p = match *filename {
            FileName::Real(ref file) => file,
            _ => return Ok(()),
        };
        if self.scx.local_sources.contains_key(&**p) {
            // We've already emitted this source
            return Ok(());
        }

        let contents = match fs::read_to_string(&p) {
            Ok(contents) => contents,
            Err(e) => {
                return Err(Error::new(e, &p));
            }
        };

        // Remove the utf-8 BOM if any
        let contents = if contents.starts_with("\u{feff}") {
            &contents[3..]
        } else {
            &contents[..]
        };

        // Create the intermediate directories
        let mut cur = self.dst.clone();
        let mut root_path = String::from("../../");
        let mut href = String::new();
        clean_path(&self.scx.src_root, &p, false, |component| {
            cur.push(component);
            root_path.push_str("../");
            href.push_str(&component.to_string_lossy());
            href.push('/');
        });
        self.scx.ensure_dir(&cur)?;
        let mut fname = p.file_name()
                         .expect("source has no filename")
                         .to_os_string();
        fname.push(".html");
        cur.push(&fname);
        href.push_str(&fname.to_string_lossy());

        let title = format!("{} -- source", cur.file_name().expect("failed to get file name")
                                               .to_string_lossy());
        let desc = format!("Source to the Rust file `{}`.", filename);
        let page = layout::Page {
            title: &title,
            css_class: "source",
            root_path: &root_path,
            static_root_path: self.scx.static_root_path.as_deref(),
            description: &desc,
            keywords: BASIC_KEYWORDS,
            resource_suffix: &self.scx.resource_suffix,
            extra_scripts: &[&format!("source-files{}", self.scx.resource_suffix)],
            static_extra_scripts: &[&format!("source-script{}", self.scx.resource_suffix)],
        };
        let v = layout::render(&self.scx.layout,
                       &page, "", |buf: &mut _| print_src(buf, &contents),
                       &self.scx.themes);
        self.scx.fs.write(&cur, v.as_bytes())?;
        self.scx.local_sources.insert(p.clone(), href);
        Ok(())
    }
}

/// Takes a path to a source file and cleans the path to it. This canonicalizes
/// things like ".." to components which preserve the "top down" hierarchy of a
/// static HTML tree. Each component in the cleaned path will be passed as an
/// argument to `f`. The very last component of the path (ie the file name) will
/// be passed to `f` if `keep_filename` is true, and ignored otherwise.
pub fn clean_path<F>(src_root: &Path, p: &Path, keep_filename: bool, mut f: F)
where
    F: FnMut(&OsStr),
{
    // make it relative, if possible
    let p = p.strip_prefix(src_root).unwrap_or(p);

    let mut iter = p.components().peekable();

    while let Some(c) = iter.next() {
        if !keep_filename && iter.peek().is_none() {
            break;
        }

        match c {
            Component::ParentDir => f("up".as_ref()),
            Component::Normal(c) => f(c),
            _ => continue,
        }
    }
}

/// Wrapper struct to render the source code of a file. This will do things like
/// adding line numbers to the left-hand side.
fn print_src(buf: &mut Buffer, s: &str) {
    let lines = s.lines().count();
    let mut cols = 0;
    let mut tmp = lines;
    while tmp > 0 {
        cols += 1;
        tmp /= 10;
    }
    write!(buf, "<pre class=\"line-numbers\">");
    for i in 1..=lines {
        write!(buf, "<span id=\"{0}\">{0:1$}</span>\n", i, cols);
    }
    write!(buf, "</pre>");
    write!(buf, "{}",
            highlight::render_with_highlighting(s, None, None, None));
}
