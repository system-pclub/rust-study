use crate::utils::{match_qpath, snippet_with_applicability, span_lint_and_sugg};
use if_chain::if_chain;
use rustc::hir;
use rustc::lint::LateContext;
use rustc_errors::Applicability;
use rustc_target::abi::LayoutOf;
use syntax::ast;

pub fn lint(cx: &LateContext<'_, '_>, expr: &hir::Expr, args: &[&[hir::Expr]], arith: &str) {
    let unwrap_arg = &args[0][1];
    let arith_lhs = &args[1][0];
    let arith_rhs = &args[1][1];

    let ty = cx.tables.expr_ty(arith_lhs);
    if !ty.is_integral() {
        return;
    }

    let mm = if let Some(mm) = is_min_or_max(cx, unwrap_arg) {
        mm
    } else {
        return;
    };

    if ty.is_signed() {
        use self::{MinMax::*, Sign::*};

        let sign = if let Some(sign) = lit_sign(arith_rhs) {
            sign
        } else {
            return;
        };

        match (arith, sign, mm) {
            ("add", Pos, Max) | ("add", Neg, Min) | ("sub", Neg, Max) | ("sub", Pos, Min) => (),
            // "mul" is omitted because lhs can be negative.
            _ => return,
        }

        let mut applicability = Applicability::MachineApplicable;
        span_lint_and_sugg(
            cx,
            super::MANUAL_SATURATING_ARITHMETIC,
            expr.span,
            "manual saturating arithmetic",
            &format!("try using `saturating_{}`", arith),
            format!(
                "{}.saturating_{}({})",
                snippet_with_applicability(cx, arith_lhs.span, "..", &mut applicability),
                arith,
                snippet_with_applicability(cx, arith_rhs.span, "..", &mut applicability),
            ),
            applicability,
        );
    } else {
        match (mm, arith) {
            (MinMax::Max, "add") | (MinMax::Max, "mul") | (MinMax::Min, "sub") => (),
            _ => return,
        }

        let mut applicability = Applicability::MachineApplicable;
        span_lint_and_sugg(
            cx,
            super::MANUAL_SATURATING_ARITHMETIC,
            expr.span,
            "manual saturating arithmetic",
            &format!("try using `saturating_{}`", arith),
            format!(
                "{}.saturating_{}({})",
                snippet_with_applicability(cx, arith_lhs.span, "..", &mut applicability),
                arith,
                snippet_with_applicability(cx, arith_rhs.span, "..", &mut applicability),
            ),
            applicability,
        );
    }
}

#[derive(PartialEq, Eq)]
enum MinMax {
    Min,
    Max,
}

fn is_min_or_max<'tcx>(cx: &LateContext<'_, 'tcx>, expr: &hir::Expr) -> Option<MinMax> {
    // `T::max_value()` `T::min_value()` inherent methods
    if_chain! {
        if let hir::ExprKind::Call(func, args) = &expr.kind;
        if args.is_empty();
        if let hir::ExprKind::Path(path) = &func.kind;
        if let hir::QPath::TypeRelative(_, segment) = path;
        then {
            match &*segment.ident.as_str() {
                "max_value" => return Some(MinMax::Max),
                "min_value" => return Some(MinMax::Min),
                _ => {}
            }
        }
    }

    let ty = cx.tables.expr_ty(expr);
    let ty_str = ty.to_string();

    // `std::T::MAX` `std::T::MIN` constants
    if let hir::ExprKind::Path(path) = &expr.kind {
        if match_qpath(path, &["core", &ty_str, "MAX"][..]) {
            return Some(MinMax::Max);
        }

        if match_qpath(path, &["core", &ty_str, "MIN"][..]) {
            return Some(MinMax::Min);
        }
    }

    // Literals
    let bits = cx.layout_of(ty).unwrap().size.bits();
    let (minval, maxval): (u128, u128) = if ty.is_signed() {
        let minval = 1 << (bits - 1);
        let mut maxval = !(1 << (bits - 1));
        if bits != 128 {
            maxval &= (1 << bits) - 1;
        }
        (minval, maxval)
    } else {
        (0, if bits == 128 { !0 } else { (1 << bits) - 1 })
    };

    let check_lit = |expr: &hir::Expr, check_min: bool| {
        if let hir::ExprKind::Lit(lit) = &expr.kind {
            if let ast::LitKind::Int(value, _) = lit.node {
                if value == maxval {
                    return Some(MinMax::Max);
                }

                if check_min && value == minval {
                    return Some(MinMax::Min);
                }
            }
        }

        None
    };

    if let r @ Some(_) = check_lit(expr, !ty.is_signed()) {
        return r;
    }

    if ty.is_signed() {
        if let hir::ExprKind::Unary(hir::UnNeg, val) = &expr.kind {
            return check_lit(val, true);
        }
    }

    None
}

#[derive(PartialEq, Eq)]
enum Sign {
    Pos,
    Neg,
}

fn lit_sign(expr: &hir::Expr) -> Option<Sign> {
    if let hir::ExprKind::Unary(hir::UnNeg, inner) = &expr.kind {
        if let hir::ExprKind::Lit(..) = &inner.kind {
            return Some(Sign::Neg);
        }
    } else if let hir::ExprKind::Lit(..) = &expr.kind {
        return Some(Sign::Pos);
    }

    None
}
