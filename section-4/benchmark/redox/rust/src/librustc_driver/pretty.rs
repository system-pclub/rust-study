//! The various pretty-printing routines.

use rustc::hir;
use rustc::hir::map as hir_map;
use rustc::hir::print as pprust_hir;
use rustc::hir::def_id::LOCAL_CRATE;
use rustc::session::Session;
use rustc::session::config::{PpMode, PpSourceMode, Input};
use rustc::ty::{self, TyCtxt};
use rustc::util::common::ErrorReported;
use rustc_mir::util::{write_mir_pretty, write_mir_graphviz};

use syntax::ast;
use syntax::print::{pprust};
use syntax_pos::FileName;

use std::cell::Cell;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub use self::PpSourceMode::*;
pub use self::PpMode::*;
use crate::abort_on_err;

// This slightly awkward construction is to allow for each PpMode to
// choose whether it needs to do analyses (which can consume the
// Session) and then pass through the session (now attached to the
// analysis results) on to the chosen pretty-printer, along with the
// `&PpAnn` object.
//
// Note that since the `&PrinterSupport` is freshly constructed on each
// call, it would not make sense to try to attach the lifetime of `self`
// to the lifetime of the `&PrinterObject`.
//
// (The `use_once_payload` is working around the current lack of once
// functions in the compiler.)

/// Constructs a `PrinterSupport` object and passes it to `f`.
fn call_with_pp_support<'tcx, A, F>(
    ppmode: &PpSourceMode,
    sess: &'tcx Session,
    tcx: Option<TyCtxt<'tcx>>,
    f: F,
) -> A
where
    F: FnOnce(&dyn PrinterSupport) -> A,
{
    match *ppmode {
        PpmNormal | PpmEveryBodyLoops | PpmExpanded => {
            let annotation = NoAnn {
                sess,
                tcx,
            };
            f(&annotation)
        }

        PpmIdentified | PpmExpandedIdentified => {
            let annotation = IdentifiedAnnotation {
                sess,
                tcx,
            };
            f(&annotation)
        }
        PpmExpandedHygiene => {
            let annotation = HygieneAnnotation {
                sess,
            };
            f(&annotation)
        }
        _ => panic!("Should use call_with_pp_support_hir"),
    }
}
fn call_with_pp_support_hir<A, F>(ppmode: &PpSourceMode, tcx: TyCtxt<'_>, f: F) -> A
where
    F: FnOnce(&dyn HirPrinterSupport<'_>, &hir::Crate) -> A,
{
    match *ppmode {
        PpmNormal => {
            let annotation = NoAnn {
                sess: tcx.sess,
                tcx: Some(tcx),
            };
            f(&annotation, tcx.hir().forest.krate())
        }

        PpmIdentified => {
            let annotation = IdentifiedAnnotation {
                sess: tcx.sess,
                tcx: Some(tcx),
            };
            f(&annotation, tcx.hir().forest.krate())
        }
        PpmTyped => {
            abort_on_err(tcx.analysis(LOCAL_CRATE), tcx.sess);

            let empty_tables = ty::TypeckTables::empty(None);
            let annotation = TypedAnnotation {
                tcx,
                tables: Cell::new(&empty_tables)
            };
            tcx.dep_graph.with_ignore(|| {
                f(&annotation, tcx.hir().forest.krate())
            })
        }
        _ => panic!("Should use call_with_pp_support"),
    }
}

trait PrinterSupport: pprust::PpAnn {
    /// Provides a uniform interface for re-extracting a reference to a
    /// `Session` from a value that now owns it.
    fn sess(&self) -> &Session;

    /// Produces the pretty-print annotation object.
    ///
    /// (Rust does not yet support upcasting from a trait object to
    /// an object for one of its super-traits.)
    fn pp_ann<'a>(&'a self) -> &'a dyn pprust::PpAnn;
}

trait HirPrinterSupport<'hir>: pprust_hir::PpAnn {
    /// Provides a uniform interface for re-extracting a reference to a
    /// `Session` from a value that now owns it.
    fn sess(&self) -> &Session;

    /// Provides a uniform interface for re-extracting a reference to an
    /// `hir_map::Map` from a value that now owns it.
    fn hir_map<'a>(&'a self) -> Option<&'a hir_map::Map<'hir>>;

    /// Produces the pretty-print annotation object.
    ///
    /// (Rust does not yet support upcasting from a trait object to
    /// an object for one of its super-traits.)
    fn pp_ann<'a>(&'a self) -> &'a dyn pprust_hir::PpAnn;

    /// Computes an user-readable representation of a path, if possible.
    fn node_path(&self, id: hir::HirId) -> Option<String> {
        self.hir_map().and_then(|map| {
            map.def_path_from_hir_id(id)
        }).map(|path| {
            path.data
                .into_iter()
                .map(|elem| elem.data.to_string())
                .collect::<Vec<_>>()
                .join("::")
        })
    }
}

struct NoAnn<'hir> {
    sess: &'hir Session,
    tcx: Option<TyCtxt<'hir>>,
}

impl<'hir> PrinterSupport for NoAnn<'hir> {
    fn sess(&self) -> &Session {
        self.sess
    }

    fn pp_ann<'a>(&'a self) -> &'a dyn pprust::PpAnn {
        self
    }
}

impl<'hir> HirPrinterSupport<'hir> for NoAnn<'hir> {
    fn sess(&self) -> &Session {
        self.sess
    }

    fn hir_map<'a>(&'a self) -> Option<&'a hir_map::Map<'hir>> {
        self.tcx.map(|tcx| tcx.hir())
    }

    fn pp_ann<'a>(&'a self) -> &'a dyn pprust_hir::PpAnn {
        self
    }
}

impl<'hir> pprust::PpAnn for NoAnn<'hir> {}
impl<'hir> pprust_hir::PpAnn for NoAnn<'hir> {
    fn nested(&self, state: &mut pprust_hir::State<'_>, nested: pprust_hir::Nested) {
        if let Some(tcx) = self.tcx {
            pprust_hir::PpAnn::nested(tcx.hir(), state, nested)
        }
    }
}

struct IdentifiedAnnotation<'hir> {
    sess: &'hir Session,
    tcx: Option<TyCtxt<'hir>>,
}

impl<'hir> PrinterSupport for IdentifiedAnnotation<'hir> {
    fn sess(&self) -> &Session {
        self.sess
    }

    fn pp_ann<'a>(&'a self) -> &'a dyn pprust::PpAnn {
        self
    }
}

impl<'hir> pprust::PpAnn for IdentifiedAnnotation<'hir> {
    fn pre(&self, s: &mut pprust::State<'_>, node: pprust::AnnNode<'_>) {
        match node {
            pprust::AnnNode::Expr(_) => s.popen(),
            _ => {}
        }
    }
    fn post(&self, s: &mut pprust::State<'_>, node: pprust::AnnNode<'_>) {
        match node {
            pprust::AnnNode::Crate(_) |
            pprust::AnnNode::Ident(_) |
            pprust::AnnNode::Name(_) => {},

            pprust::AnnNode::Item(item) => {
                s.s.space();
                s.synth_comment(item.id.to_string())
            }
            pprust::AnnNode::SubItem(id) => {
                s.s.space();
                s.synth_comment(id.to_string())
            }
            pprust::AnnNode::Block(blk) => {
                s.s.space();
                s.synth_comment(format!("block {}", blk.id))
            }
            pprust::AnnNode::Expr(expr) => {
                s.s.space();
                s.synth_comment(expr.id.to_string());
                s.pclose()
            }
            pprust::AnnNode::Pat(pat) => {
                s.s.space();
                s.synth_comment(format!("pat {}", pat.id));
            }
        }
    }
}

impl<'hir> HirPrinterSupport<'hir> for IdentifiedAnnotation<'hir> {
    fn sess(&self) -> &Session {
        self.sess
    }

    fn hir_map<'a>(&'a self) -> Option<&'a hir_map::Map<'hir>> {
        self.tcx.map(|tcx| tcx.hir())
    }

    fn pp_ann<'a>(&'a self) -> &'a dyn pprust_hir::PpAnn {
        self
    }
}

impl<'hir> pprust_hir::PpAnn for IdentifiedAnnotation<'hir> {
    fn nested(&self, state: &mut pprust_hir::State<'_>, nested: pprust_hir::Nested) {
        if let Some(ref tcx) = self.tcx {
            pprust_hir::PpAnn::nested(tcx.hir(), state, nested)
        }
    }
    fn pre(&self, s: &mut pprust_hir::State<'_>, node: pprust_hir::AnnNode<'_>) {
        match node {
            pprust_hir::AnnNode::Expr(_) => s.popen(),
            _ => {}
        }
    }
    fn post(&self, s: &mut pprust_hir::State<'_>, node: pprust_hir::AnnNode<'_>) {
        match node {
            pprust_hir::AnnNode::Name(_) => {},
            pprust_hir::AnnNode::Item(item) => {
                s.s.space();
                s.synth_comment(format!("hir_id: {}", item.hir_id));
            }
            pprust_hir::AnnNode::SubItem(id) => {
                s.s.space();
                s.synth_comment(id.to_string());
            }
            pprust_hir::AnnNode::Block(blk) => {
                s.s.space();
                s.synth_comment(format!("block hir_id: {}", blk.hir_id));
            }
            pprust_hir::AnnNode::Expr(expr) => {
                s.s.space();
                s.synth_comment(format!("expr hir_id: {}", expr.hir_id));
                s.pclose();
            }
            pprust_hir::AnnNode::Pat(pat) => {
                s.s.space();
                s.synth_comment(format!("pat hir_id: {}", pat.hir_id));
            }
            pprust_hir::AnnNode::Arm(arm) => {
                s.s.space();
                s.synth_comment(format!("arm hir_id: {}", arm.hir_id));
            }
        }
    }
}

struct HygieneAnnotation<'a> {
    sess: &'a Session
}

impl<'a> PrinterSupport for HygieneAnnotation<'a> {
    fn sess(&self) -> &Session {
        self.sess
    }

    fn pp_ann(&self) -> &dyn pprust::PpAnn {
        self
    }
}

impl<'a> pprust::PpAnn for HygieneAnnotation<'a> {
    fn post(&self, s: &mut pprust::State<'_>, node: pprust::AnnNode<'_>) {
        match node {
            pprust::AnnNode::Ident(&ast::Ident { name, span }) => {
                s.s.space();
                s.synth_comment(format!("{}{:?}", name.as_u32(), span.ctxt()))
            }
            pprust::AnnNode::Name(&name) => {
                s.s.space();
                s.synth_comment(name.as_u32().to_string())
            }
            pprust::AnnNode::Crate(_) => {
                s.s.hardbreak();
                let verbose = self.sess.verbose();
                s.synth_comment(syntax_pos::hygiene::debug_hygiene_data(verbose));
                s.s.hardbreak_if_not_bol();
            }
            _ => {}
        }
    }
}

struct TypedAnnotation<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    tables: Cell<&'a ty::TypeckTables<'tcx>>,
}

impl<'b, 'tcx> HirPrinterSupport<'tcx> for TypedAnnotation<'b, 'tcx> {
    fn sess(&self) -> &Session {
        &self.tcx.sess
    }

    fn hir_map<'a>(&'a self) -> Option<&'a hir_map::Map<'tcx>> {
        Some(&self.tcx.hir())
    }

    fn pp_ann<'a>(&'a self) -> &'a dyn pprust_hir::PpAnn {
        self
    }

    fn node_path(&self, id: hir::HirId) -> Option<String> {
        Some(self.tcx.def_path_str(self.tcx.hir().local_def_id(id)))
    }
}

impl<'a, 'tcx> pprust_hir::PpAnn for TypedAnnotation<'a, 'tcx> {
    fn nested(&self, state: &mut pprust_hir::State<'_>, nested: pprust_hir::Nested) {
        let old_tables = self.tables.get();
        if let pprust_hir::Nested::Body(id) = nested {
            self.tables.set(self.tcx.body_tables(id));
        }
        pprust_hir::PpAnn::nested(self.tcx.hir(), state, nested);
        self.tables.set(old_tables);
    }
    fn pre(&self, s: &mut pprust_hir::State<'_>, node: pprust_hir::AnnNode<'_>) {
        match node {
            pprust_hir::AnnNode::Expr(_) => s.popen(),
            _ => {}
        }
    }
    fn post(&self, s: &mut pprust_hir::State<'_>, node: pprust_hir::AnnNode<'_>) {
        match node {
            pprust_hir::AnnNode::Expr(expr) => {
                s.s.space();
                s.s.word("as");
                s.s.space();
                s.s.word(self.tables.get().expr_ty(expr).to_string());
                s.pclose();
            }
            _ => {},
        }
    }
}

fn get_source(input: &Input, sess: &Session) -> (String, FileName) {
    let src_name = input.source_name();
    let src = String::clone(&sess.source_map()
        .get_source_file(&src_name)
        .unwrap()
        .src
        .as_ref()
        .unwrap());
    (src, src_name)
}

fn write_output(out: Vec<u8>, ofile: Option<&Path>) {
    match ofile {
        None => print!("{}", String::from_utf8(out).unwrap()),
        Some(p) => {
            match File::create(p) {
                Ok(mut w) => w.write_all(&out).unwrap(),
                Err(e) => panic!("print-print failed to open {} due to {}", p.display(), e),
            }
        }
    }
}

pub fn print_after_parsing(sess: &Session,
                           input: &Input,
                           krate: &ast::Crate,
                           ppm: PpMode,
                           ofile: Option<&Path>) {
    let (src, src_name) = get_source(input, sess);

    let mut out = String::new();

    if let PpmSource(s) = ppm {
        // Silently ignores an identified node.
        let out = &mut out;
        call_with_pp_support(&s, sess, None, move |annotation| {
            debug!("pretty printing source code {:?}", s);
            let sess = annotation.sess();
            *out = pprust::print_crate(sess.source_map(),
                                &sess.parse_sess,
                                krate,
                                src_name,
                                src,
                                annotation.pp_ann(),
                                false)
        })
    } else {
        unreachable!();
    };

    write_output(out.into_bytes(), ofile);
}

pub fn print_after_hir_lowering<'tcx>(
    tcx: TyCtxt<'tcx>,
    input: &Input,
    krate: &ast::Crate,
    ppm: PpMode,
    ofile: Option<&Path>,
) {
    if ppm.needs_analysis() {
        abort_on_err(print_with_analysis(
            tcx,
            ppm,
            ofile
        ), tcx.sess);
        return;
    }

    let (src, src_name) = get_source(input, tcx.sess);

    let mut out = String::new();

    match ppm {
            PpmSource(s) => {
                // Silently ignores an identified node.
                let out = &mut out;
                let src = src.clone();
                call_with_pp_support(&s, tcx.sess, Some(tcx), move |annotation| {
                    debug!("pretty printing source code {:?}", s);
                    let sess = annotation.sess();
                    *out = pprust::print_crate(sess.source_map(),
                                        &sess.parse_sess,
                                        krate,
                                        src_name,
                                        src,
                                        annotation.pp_ann(),
                                        true)
                })
            }

            PpmHir(s) => {
                let out = &mut out;
                let src = src.clone();
                call_with_pp_support_hir(&s, tcx, move |annotation, krate| {
                    debug!("pretty printing source code {:?}", s);
                    let sess = annotation.sess();
                    *out = pprust_hir::print_crate(sess.source_map(),
                                            &sess.parse_sess,
                                            krate,
                                            src_name,
                                            src,
                                            annotation.pp_ann())
                })
            }

            PpmHirTree(s) => {
                let out = &mut out;
                call_with_pp_support_hir(&s, tcx, move |_annotation, krate| {
                    debug!("pretty printing source code {:?}", s);
                    *out = format!("{:#?}", krate);
                });
            }

            _ => unreachable!(),
        }

    write_output(out.into_bytes(), ofile);
}

// In an ideal world, this would be a public function called by the driver after
// analysis is performed. However, we want to call `phase_3_run_analysis_passes`
// with a different callback than the standard driver, so that isn't easy.
// Instead, we call that function ourselves.
fn print_with_analysis(
    tcx: TyCtxt<'_>,
    ppm: PpMode,
    ofile: Option<&Path>,
) -> Result<(), ErrorReported> {
    let mut out = Vec::new();

    tcx.analysis(LOCAL_CRATE)?;

    match ppm {
        PpmMir | PpmMirCFG => {
            match ppm {
                PpmMir => write_mir_pretty(tcx, None, &mut out),
                PpmMirCFG => write_mir_graphviz(tcx, None, &mut out),
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    }.unwrap();

    write_output(out, ofile);

    Ok(())
}
