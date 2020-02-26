use crate::utils::paths;
use crate::utils::{
    is_expn_of, last_path_segment, match_def_path, match_function_call, match_type, snippet, span_lint_and_then,
    walk_ptrs_ty,
};
use if_chain::if_chain;
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintContext, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;
use syntax::ast::LitKind;
use syntax::source_map::Span;

declare_clippy_lint! {
    /// **What it does:** Checks for the use of `format!("string literal with no
    /// argument")` and `format!("{}", foo)` where `foo` is a string.
    ///
    /// **Why is this bad?** There is no point of doing that. `format!("foo")` can
    /// be replaced by `"foo".to_owned()` if you really need a `String`. The even
    /// worse `&format!("foo")` is often encountered in the wild. `format!("{}",
    /// foo)` can be replaced by `foo.clone()` if `foo: String` or `foo.to_owned()`
    /// if `foo: &str`.
    ///
    /// **Known problems:** None.
    ///
    /// **Examples:**
    /// ```rust
    /// # let foo = "foo";
    /// format!("foo");
    /// format!("{}", foo);
    /// ```
    pub USELESS_FORMAT,
    complexity,
    "useless use of `format!`"
}

declare_lint_pass!(UselessFormat => [USELESS_FORMAT]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for UselessFormat {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        let span = match is_expn_of(expr.span, "format") {
            Some(s) if !s.from_expansion() => s,
            _ => return,
        };

        // Operate on the only argument of `alloc::fmt::format`.
        if let Some(sugg) = on_new_v1(cx, expr) {
            span_useless_format(cx, span, "consider using .to_string()", sugg);
        } else if let Some(sugg) = on_new_v1_fmt(cx, expr) {
            span_useless_format(cx, span, "consider using .to_string()", sugg);
        }
    }
}

fn span_useless_format<T: LintContext>(cx: &T, span: Span, help: &str, mut sugg: String) {
    let to_replace = span.source_callsite();

    // The callsite span contains the statement semicolon for some reason.
    let snippet = snippet(cx, to_replace, "..");
    if snippet.ends_with(';') {
        sugg.push(';');
    }

    span_lint_and_then(cx, USELESS_FORMAT, span, "useless use of `format!`", |db| {
        db.span_suggestion(
            to_replace,
            help,
            sugg,
            Applicability::MachineApplicable, // snippet
        );
    });
}

fn on_argumentv1_new<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr, arms: &'tcx [Arm]) -> Option<String> {
    if_chain! {
        if let ExprKind::AddrOf(_, ref format_args) = expr.kind;
        if let ExprKind::Array(ref elems) = arms[0].body.kind;
        if elems.len() == 1;
        if let Some(args) = match_function_call(cx, &elems[0], &paths::FMT_ARGUMENTV1_NEW);
        // matches `core::fmt::Display::fmt`
        if args.len() == 2;
        if let ExprKind::Path(ref qpath) = args[1].kind;
        if let Some(did) = cx.tables.qpath_res(qpath, args[1].hir_id).opt_def_id();
        if match_def_path(cx, did, &paths::DISPLAY_FMT_METHOD);
        // check `(arg0,)` in match block
        if let PatKind::Tuple(ref pats, None) = arms[0].pat.kind;
        if pats.len() == 1;
        then {
            let ty = walk_ptrs_ty(cx.tables.pat_ty(&pats[0]));
            if ty.kind != rustc::ty::Str && !match_type(cx, ty, &paths::STRING) {
                return None;
            }
            if let ExprKind::Lit(ref lit) = format_args.kind {
                if let LitKind::Str(ref s, _) = lit.node {
                    return Some(format!("{:?}.to_string()", s.as_str()));
                }
            } else {
                let snip = snippet(cx, format_args.span, "<arg>");
                if let ExprKind::MethodCall(ref path, _, _) = format_args.kind {
                    if path.ident.name == sym!(to_string) {
                        return Some(format!("{}", snip));
                    }
                } else if let ExprKind::Binary(..) = format_args.kind {
                    return Some(format!("{}", snip));
                }
                return Some(format!("{}.to_string()", snip));
            }
        }
    }
    None
}

fn on_new_v1<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) -> Option<String> {
    if_chain! {
        if let Some(args) = match_function_call(cx, expr, &paths::FMT_ARGUMENTS_NEW_V1);
        if args.len() == 2;
        // Argument 1 in `new_v1()`
        if let ExprKind::AddrOf(_, ref arr) = args[0].kind;
        if let ExprKind::Array(ref pieces) = arr.kind;
        if pieces.len() == 1;
        if let ExprKind::Lit(ref lit) = pieces[0].kind;
        if let LitKind::Str(ref s, _) = lit.node;
        // Argument 2 in `new_v1()`
        if let ExprKind::AddrOf(_, ref arg1) = args[1].kind;
        if let ExprKind::Match(ref matchee, ref arms, MatchSource::Normal) = arg1.kind;
        if arms.len() == 1;
        if let ExprKind::Tup(ref tup) = matchee.kind;
        then {
            // `format!("foo")` expansion contains `match () { () => [], }`
            if tup.is_empty() {
                return Some(format!("{:?}.to_string()", s.as_str()));
            } else if s.as_str().is_empty() {
                return on_argumentv1_new(cx, &tup[0], arms);
            }
        }
    }
    None
}

fn on_new_v1_fmt<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) -> Option<String> {
    if_chain! {
        if let Some(args) = match_function_call(cx, expr, &paths::FMT_ARGUMENTS_NEW_V1_FORMATTED);
        if args.len() == 3;
        if check_unformatted(&args[2]);
        // Argument 1 in `new_v1_formatted()`
        if let ExprKind::AddrOf(_, ref arr) = args[0].kind;
        if let ExprKind::Array(ref pieces) = arr.kind;
        if pieces.len() == 1;
        if let ExprKind::Lit(ref lit) = pieces[0].kind;
        if let LitKind::Str(..) = lit.node;
        // Argument 2 in `new_v1_formatted()`
        if let ExprKind::AddrOf(_, ref arg1) = args[1].kind;
        if let ExprKind::Match(ref matchee, ref arms, MatchSource::Normal) = arg1.kind;
        if arms.len() == 1;
        if let ExprKind::Tup(ref tup) = matchee.kind;
        then {
            return on_argumentv1_new(cx, &tup[0], arms);
        }
    }
    None
}

/// Checks if the expression matches
/// ```rust,ignore
/// &[_ {
///    format: _ {
///         width: _::Implied,
///         precision: _::Implied,
///         ...
///    },
///    ...,
/// }]
/// ```
fn check_unformatted(expr: &Expr) -> bool {
    if_chain! {
        if let ExprKind::AddrOf(_, ref expr) = expr.kind;
        if let ExprKind::Array(ref exprs) = expr.kind;
        if exprs.len() == 1;
        // struct `core::fmt::rt::v1::Argument`
        if let ExprKind::Struct(_, ref fields, _) = exprs[0].kind;
        if let Some(format_field) = fields.iter().find(|f| f.ident.name == sym!(format));
        // struct `core::fmt::rt::v1::FormatSpec`
        if let ExprKind::Struct(_, ref fields, _) = format_field.expr.kind;
        if let Some(precision_field) = fields.iter().find(|f| f.ident.name == sym!(precision));
        if let ExprKind::Path(ref precision_path) = precision_field.expr.kind;
        if last_path_segment(precision_path).ident.name == sym!(Implied);
        if let Some(width_field) = fields.iter().find(|f| f.ident.name == sym!(width));
        if let ExprKind::Path(ref width_qpath) = width_field.expr.kind;
        if last_path_segment(width_qpath).ident.name == sym!(Implied);
        then {
            return true;
        }
    }

    false
}
