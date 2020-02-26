//! Give useful errors and suggestions to users when an item can't be
//! found or is otherwise invalid.

use crate::check::FnCtxt;
use crate::middle::lang_items::FnOnceTraitLangItem;
use crate::namespace::Namespace;
use crate::util::nodemap::FxHashSet;
use errors::{Applicability, DiagnosticBuilder, pluralize};
use rustc::hir::{self, ExprKind, Node, QPath};
use rustc::hir::def::{Res, DefKind};
use rustc::hir::def_id::{CRATE_DEF_INDEX, LOCAL_CRATE, DefId};
use rustc::hir::map as hir_map;
use rustc::infer::type_variable::{TypeVariableOrigin, TypeVariableOriginKind};
use rustc::traits::Obligation;
use rustc::ty::{self, Ty, TyCtxt, ToPolyTraitRef, ToPredicate, TypeFoldable};
use rustc::ty::print::with_crate_prefix;
use syntax_pos::{Span, FileName};
use syntax::ast;
use syntax::util::lev_distance;

use rustc_error_codes::*;

use std::cmp::Ordering;

use super::{MethodError, NoMatchData, CandidateSource};
use super::probe::Mode;

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    fn is_fn_ty(&self, ty: Ty<'tcx>, span: Span) -> bool {
        let tcx = self.tcx;
        match ty.kind {
            // Not all of these (e.g., unsafe fns) implement `FnOnce`,
            // so we look for these beforehand.
            ty::Closure(..) |
            ty::FnDef(..) |
            ty::FnPtr(_) => true,
            // If it's not a simple function, look for things which implement `FnOnce`.
            _ => {
                let fn_once = match tcx.lang_items().require(FnOnceTraitLangItem) {
                    Ok(fn_once) => fn_once,
                    Err(..) => return false,
                };

                self.autoderef(span, ty).any(|(ty, _)| {
                    self.probe(|_| {
                        let fn_once_substs = tcx.mk_substs_trait(ty, &[
                            self.next_ty_var(TypeVariableOrigin {
                                kind: TypeVariableOriginKind::MiscVariable,
                                span,
                            }).into()
                        ]);
                        let trait_ref = ty::TraitRef::new(fn_once, fn_once_substs);
                        let poly_trait_ref = trait_ref.to_poly_trait_ref();
                        let obligation =
                            Obligation::misc(span,
                                             self.body_id,
                                             self.param_env,
                                             poly_trait_ref.to_predicate());
                        self.predicate_may_hold(&obligation)
                    })
                })
            }
        }
    }

    pub fn report_method_error<'b>(
        &self,
        span: Span,
        rcvr_ty: Ty<'tcx>,
        item_name: ast::Ident,
        source: SelfSource<'b>,
        error: MethodError<'tcx>,
        args: Option<&'tcx [hir::Expr]>,
    ) -> Option<DiagnosticBuilder<'_>> {
        let orig_span = span;
        let mut span = span;
        // Avoid suggestions when we don't know what's going on.
        if rcvr_ty.references_error() {
            return None;
        }

        let print_disambiguation_help = |
            err: &mut DiagnosticBuilder<'_>,
            trait_name: String,
        | {
            err.help(&format!(
                "to disambiguate the method call, write `{}::{}({}{})` instead",
                trait_name,
                item_name,
                if rcvr_ty.is_region_ptr() && args.is_some() {
                    if rcvr_ty.is_mutable_ptr() {
                        "&mut "
                    } else {
                        "&"
                    }
                } else {
                    ""
                },
                args.map(|arg| arg
                    .iter()
                    .map(|arg| self.tcx.sess.source_map().span_to_snippet(arg.span)
                        .unwrap_or_else(|_| "...".to_owned()))
                    .collect::<Vec<_>>()
                    .join(", ")
                ).unwrap_or_else(|| "...".to_owned())
            ));
        };

        let report_candidates = |
            span: Span,
            err: &mut DiagnosticBuilder<'_>,
            mut sources: Vec<CandidateSource>,
        | {
            sources.sort();
            sources.dedup();
            // Dynamic limit to avoid hiding just one candidate, which is silly.
            let limit = if sources.len() == 5 { 5 } else { 4 };

            for (idx, source) in sources.iter().take(limit).enumerate() {
                match *source {
                    CandidateSource::ImplSource(impl_did) => {
                        // Provide the best span we can. Use the item, if local to crate, else
                        // the impl, if local to crate (item may be defaulted), else nothing.
                        let item = match self.associated_item(
                            impl_did,
                            item_name,
                            Namespace::Value,
                        ).or_else(|| {
                            let impl_trait_ref = self.tcx.impl_trait_ref(impl_did)?;
                            self.associated_item(
                                impl_trait_ref.def_id,
                                item_name,
                                Namespace::Value,
                            )
                        }) {
                            Some(item) => item,
                            None => continue,
                        };
                        let note_span = self.tcx.hir().span_if_local(item.def_id).or_else(|| {
                            self.tcx.hir().span_if_local(impl_did)
                        });

                        let impl_ty = self.impl_self_ty(span, impl_did).ty;

                        let insertion = match self.tcx.impl_trait_ref(impl_did) {
                            None => String::new(),
                            Some(trait_ref) => {
                                format!(" of the trait `{}`",
                                        self.tcx.def_path_str(trait_ref.def_id))
                            }
                        };

                        let note_str = if sources.len() > 1 {
                            format!("candidate #{} is defined in an impl{} for the type `{}`",
                                    idx + 1,
                                    insertion,
                                    impl_ty)
                        } else {
                            format!("the candidate is defined in an impl{} for the type `{}`",
                                    insertion,
                                    impl_ty)
                        };
                        if let Some(note_span) = note_span {
                            // We have a span pointing to the method. Show note with snippet.
                            err.span_note(self.tcx.sess.source_map().def_span(note_span),
                                          &note_str);
                        } else {
                            err.note(&note_str);
                        }
                        if let Some(trait_ref) = self.tcx.impl_trait_ref(impl_did) {
                            print_disambiguation_help(err, self.tcx.def_path_str(trait_ref.def_id));
                        }
                    }
                    CandidateSource::TraitSource(trait_did) => {
                        let item = match self.associated_item(
                            trait_did,
                            item_name,
                            Namespace::Value)
                        {
                            Some(item) => item,
                            None => continue,
                        };
                        let item_span = self.tcx.sess.source_map()
                            .def_span(self.tcx.def_span(item.def_id));
                        if sources.len() > 1 {
                            span_note!(err,
                                       item_span,
                                       "candidate #{} is defined in the trait `{}`",
                                       idx + 1,
                                       self.tcx.def_path_str(trait_did));
                        } else {
                            span_note!(err,
                                       item_span,
                                       "the candidate is defined in the trait `{}`",
                                       self.tcx.def_path_str(trait_did));
                        }
                        print_disambiguation_help(err, self.tcx.def_path_str(trait_did));
                    }
                }
            }
            if sources.len() > limit {
                err.note(&format!("and {} others", sources.len() - limit));
            }
        };

        match error {
            MethodError::NoMatch(NoMatchData {
                static_candidates: static_sources,
                unsatisfied_predicates,
                out_of_scope_traits,
                lev_candidate,
                mode,
            }) => {
                let tcx = self.tcx;

                let actual = self.resolve_vars_if_possible(&rcvr_ty);
                let ty_str = self.ty_to_string(actual);
                let is_method = mode == Mode::MethodCall;
                let item_kind = if is_method {
                    "method"
                } else if actual.is_enum() {
                    "variant or associated item"
                } else {
                    match (item_name.as_str().chars().next(), actual.is_fresh_ty()) {
                        (Some(name), false) if name.is_lowercase() => {
                            "function or associated item"
                        }
                        (Some(_), false) => "associated item",
                        (Some(_), true) | (None, false) => {
                            "variant or associated item"
                        }
                        (None, true) => "variant",
                    }
                };
                let mut err = if !actual.references_error() {
                    // Suggest clamping down the type if the method that is being attempted to
                    // be used exists at all, and the type is an ambiguous numeric type
                    // ({integer}/{float}).
                    let mut candidates = all_traits(self.tcx)
                        .into_iter()
                        .filter_map(|info|
                            self.associated_item(info.def_id, item_name, Namespace::Value)
                        );
                    if let (true, false, SelfSource::MethodCall(expr), Some(_)) =
                           (actual.is_numeric(),
                            actual.has_concrete_skeleton(),
                            source,
                            candidates.next()) {
                        let mut err = struct_span_err!(
                            tcx.sess,
                            span,
                            E0689,
                            "can't call {} `{}` on ambiguous numeric type `{}`",
                            item_kind,
                            item_name,
                            ty_str
                        );
                        let concrete_type = if actual.is_integral() {
                            "i32"
                        } else {
                            "f32"
                        };
                        match expr.kind {
                            ExprKind::Lit(ref lit) => {
                                // numeric literal
                                let snippet = tcx.sess.source_map().span_to_snippet(lit.span)
                                    .unwrap_or_else(|_| "<numeric literal>".to_owned());

                                err.span_suggestion(
                                    lit.span,
                                    &format!("you must specify a concrete type for \
                                              this numeric value, like `{}`", concrete_type),
                                    format!("{}_{}", snippet, concrete_type),
                                    Applicability::MaybeIncorrect,
                                );
                            }
                            ExprKind::Path(ref qpath) => {
                                // local binding
                                if let &QPath::Resolved(_, ref path) = &qpath {
                                    if let hir::def::Res::Local(hir_id) = path.res {
                                        let span = tcx.hir().span(hir_id);
                                        let snippet = tcx.sess.source_map().span_to_snippet(span);
                                        let filename = tcx.sess.source_map().span_to_filename(span);

                                        let parent_node = self.tcx.hir().get(
                                            self.tcx.hir().get_parent_node(hir_id),
                                        );
                                        let msg = format!(
                                            "you must specify a type for this binding, like `{}`",
                                            concrete_type,
                                        );

                                        match (filename, parent_node, snippet) {
                                            (FileName::Real(_), Node::Local(hir::Local {
                                                source: hir::LocalSource::Normal,
                                                ty,
                                                ..
                                            }), Ok(ref snippet)) => {
                                                err.span_suggestion(
                                                    // account for `let x: _ = 42;`
                                                    //                  ^^^^
                                                    span.to(ty.as_ref().map(|ty| ty.span)
                                                        .unwrap_or(span)),
                                                    &msg,
                                                    format!("{}: {}", snippet, concrete_type),
                                                    Applicability::MaybeIncorrect,
                                                );
                                            }
                                            _ => {
                                                err.span_label(span, msg);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                        err.emit();
                        return None;
                    } else {
                        span = item_name.span;
                        let mut err = struct_span_err!(
                            tcx.sess,
                            span,
                            E0599,
                            "no {} named `{}` found for type `{}` in the current scope",
                            item_kind,
                            item_name,
                            ty_str
                        );
                        if let Some(span) = tcx.sess.confused_type_with_std_module.borrow()
                            .get(&span)
                        {
                            if let Ok(snippet) = tcx.sess.source_map().span_to_snippet(*span) {
                                err.span_suggestion(
                                    *span,
                                    "you are looking for the module in `std`, \
                                     not the primitive type",
                                    format!("std::{}", snippet),
                                    Applicability::MachineApplicable,
                                );
                            }
                        }
                        if let ty::RawPtr(_) = &actual.kind {
                            err.note("try using `<*const T>::as_ref()` to get a reference to the \
                                      type behind the pointer: https://doc.rust-lang.org/std/\
                                      primitive.pointer.html#method.as_ref");
                            err.note("using `<*const T>::as_ref()` on a pointer \
                                      which is unaligned or points to invalid \
                                      or uninitialized memory is undefined behavior");
                        }
                        err
                    }
                } else {
                    tcx.sess.diagnostic().struct_dummy()
                };

                if let Some(def) = actual.ty_adt_def() {
                    if let Some(full_sp) = tcx.hir().span_if_local(def.did) {
                        let def_sp = tcx.sess.source_map().def_span(full_sp);
                        err.span_label(def_sp, format!("{} `{}` not found {}",
                                                       item_kind,
                                                       item_name,
                                                       if def.is_enum() && !is_method {
                                                           "here"
                                                       } else {
                                                           "for this"
                                                       }));
                    }
                }

                // If the method name is the name of a field with a function or closure type,
                // give a helping note that it has to be called as `(x.f)(...)`.
                if let SelfSource::MethodCall(expr) = source {
                    let field_receiver = self
                        .autoderef(span, rcvr_ty)
                        .find_map(|(ty, _)| match ty.kind {
                            ty::Adt(def, substs) if !def.is_enum() => {
                                let variant = &def.non_enum_variant();
                                self.tcx.find_field_index(item_name, variant).map(|index| {
                                    let field = &variant.fields[index];
                                    let field_ty = field.ty(tcx, substs);
                                    (field, field_ty)
                                })
                            }
                            _ => None,
                        });

                    if let Some((field, field_ty)) = field_receiver {
                        let scope = self.tcx.hir().get_module_parent(self.body_id);
                        let is_accessible = field.vis.is_accessible_from(scope, self.tcx);

                        if is_accessible {
                            if self.is_fn_ty(&field_ty, span) {
                                let expr_span = expr.span.to(item_name.span);
                                err.multipart_suggestion(
                                    &format!(
                                        "to call the function stored in `{}`, \
                                         surround the field access with parentheses",
                                        item_name,
                                    ),
                                    vec![
                                        (expr_span.shrink_to_lo(), '('.to_string()),
                                        (expr_span.shrink_to_hi(), ')'.to_string()),
                                    ],
                                    Applicability::MachineApplicable,
                                );
                            } else {
                                let call_expr = self.tcx.hir().expect_expr(
                                    self.tcx.hir().get_parent_node(expr.hir_id),
                                );

                                if let Some(span) = call_expr.span.trim_start(item_name.span) {
                                    err.span_suggestion(
                                        span,
                                        "remove the arguments",
                                        String::new(),
                                        Applicability::MaybeIncorrect,
                                    );
                                }
                            }
                        }

                        let field_kind = if is_accessible {
                            "field"
                        } else {
                            "private field"
                        };
                        err.span_label(item_name.span, format!("{}, not a method", field_kind));
                    } else if lev_candidate.is_none() && static_sources.is_empty() {
                        err.span_label(span, format!("{} not found in `{}`", item_kind, ty_str));
                        self.tcx.sess.trait_methods_not_found.borrow_mut().insert(orig_span);
                    }
                } else {
                    err.span_label(span, format!("{} not found in `{}`", item_kind, ty_str));
                    self.tcx.sess.trait_methods_not_found.borrow_mut().insert(orig_span);
                }

                if self.is_fn_ty(&rcvr_ty, span) {
                    macro_rules! report_function {
                        ($span:expr, $name:expr) => {
                            err.note(&format!("{} is a function, perhaps you wish to call it",
                                              $name));
                        }
                    }

                    if let SelfSource::MethodCall(expr) = source {
                        if let Ok(expr_string) = tcx.sess.source_map().span_to_snippet(expr.span) {
                            report_function!(expr.span, expr_string);
                        } else if let ExprKind::Path(QPath::Resolved(_, ref path)) =
                            expr.kind
                        {
                            if let Some(segment) = path.segments.last() {
                                report_function!(expr.span, segment.ident);
                            }
                        }
                    }
                }

                if !static_sources.is_empty() {
                    err.note("found the following associated functions; to be used as methods, \
                              functions must have a `self` parameter");
                    err.span_label(span, "this is an associated function, not a method");
                }
                if static_sources.len() == 1 {
                    let ty_str = if let Some(CandidateSource::ImplSource(
                        impl_did,
                    )) = static_sources.get(0) {
                        // When the "method" is resolved through dereferencing, we really want the
                        // original type that has the associated function for accurate suggestions.
                        // (#61411)
                        let ty = self.impl_self_ty(span, *impl_did).ty;
                        match (&ty.peel_refs().kind, &actual.peel_refs().kind) {
                            (ty::Adt(def, _), ty::Adt(def_actual, _)) if def == def_actual => {
                                // Use `actual` as it will have more `substs` filled in.
                                self.ty_to_value_string(actual.peel_refs())
                            }
                            _ => self.ty_to_value_string(ty.peel_refs()),
                        }
                    } else {
                        self.ty_to_value_string(actual.peel_refs())
                    };
                    if let SelfSource::MethodCall(expr) = source {
                        err.span_suggestion(
                            expr.span.to(span),
                            "use associated function syntax instead",
                            format!("{}::{}", ty_str, item_name),
                            Applicability::MachineApplicable,
                        );
                    } else {
                        err.help(&format!(
                            "try with `{}::{}`",
                            ty_str,
                            item_name,
                        ));
                    }

                    report_candidates(span, &mut err, static_sources);
                } else if static_sources.len() > 1 {
                    report_candidates(span, &mut err, static_sources);
                }

                if !unsatisfied_predicates.is_empty() {
                    let mut bound_list = unsatisfied_predicates.iter()
                        .map(|p| format!("`{} : {}`", p.self_ty(), p))
                        .collect::<Vec<_>>();
                    bound_list.sort();
                    bound_list.dedup();  // #35677
                    let bound_list = bound_list.join("\n");
                    err.note(&format!("the method `{}` exists but the following trait bounds \
                                       were not satisfied:\n{}",
                                      item_name,
                                      bound_list));
                }

                if actual.is_numeric() && actual.is_fresh() {

                } else {
                    self.suggest_traits_to_import(&mut err,
                                                  span,
                                                  rcvr_ty,
                                                  item_name,
                                                  source,
                                                  out_of_scope_traits);
                }

                if actual.is_enum() {
                    let adt_def = actual.ty_adt_def().expect("enum is not an ADT");
                    if let Some(suggestion) = lev_distance::find_best_match_for_name(
                        adt_def.variants.iter().map(|s| &s.ident.name),
                        &item_name.as_str(),
                        None,
                    ) {
                        err.span_suggestion(
                            span,
                            "there is a variant with a similar name",
                            suggestion.to_string(),
                            Applicability::MaybeIncorrect,
                        );
                    }
                }

                let mut fallback_span = true;
                let msg = "remove this method call";
                if item_name.as_str() == "as_str" && actual.peel_refs().is_str() {
                    if let SelfSource::MethodCall(expr) = source {
                        let call_expr = self.tcx.hir().expect_expr(
                            self.tcx.hir().get_parent_node(expr.hir_id),
                        );
                        if let Some(span) = call_expr.span.trim_start(expr.span) {
                            err.span_suggestion(
                                span,
                                msg,
                                String::new(),
                                Applicability::MachineApplicable,
                            );
                            fallback_span = false;
                        }
                    }
                    if fallback_span {
                        err.span_label(span, msg);
                    }
                } else if let Some(lev_candidate) = lev_candidate {
                    let def_kind = lev_candidate.def_kind();
                    err.span_suggestion(
                        span,
                        &format!(
                            "there is {} {} with a similar name",
                            def_kind.article(),
                            def_kind.descr(lev_candidate.def_id),
                        ),
                        lev_candidate.ident.to_string(),
                        Applicability::MaybeIncorrect,
                    );
                }

                return Some(err);
            }

            MethodError::Ambiguity(sources) => {
                let mut err = struct_span_err!(self.sess(),
                                               span,
                                               E0034,
                                               "multiple applicable items in scope");
                err.span_label(span, format!("multiple `{}` found", item_name));

                report_candidates(span, &mut err, sources);
                err.emit();
            }

            MethodError::PrivateMatch(kind, def_id, out_of_scope_traits) => {
                let mut err = struct_span_err!(self.tcx.sess, span, E0624,
                                               "{} `{}` is private", kind.descr(def_id), item_name);
                self.suggest_valid_traits(&mut err, out_of_scope_traits);
                err.emit();
            }

            MethodError::IllegalSizedBound(candidates, needs_mut) => {
                let msg = format!("the `{}` method cannot be invoked on a trait object", item_name);
                let mut err = self.sess().struct_span_err(span, &msg);
                if !candidates.is_empty() {
                    let help = format!(
                        "{an}other candidate{s} {were} found in the following trait{s}, perhaps \
                         add a `use` for {one_of_them}:",
                        an = if candidates.len() == 1 {"an" } else { "" },
                        s = pluralize!(candidates.len()),
                        were = if candidates.len() == 1 { "was" } else { "were" },
                        one_of_them = if candidates.len() == 1 {
                            "it"
                        } else {
                            "one_of_them"
                        },
                    );
                    self.suggest_use_candidates(&mut err, help, candidates);
                }
                if let ty::Ref(region, t_type, mutability) = rcvr_ty.kind {
                    if needs_mut {
                        let trait_type = self.tcx.mk_ref(region, ty::TypeAndMut {
                            ty: t_type,
                            mutbl: mutability.invert(),
                        });
                        err.note(&format!("you need `{}` instead of `{}`", trait_type, rcvr_ty));
                    }
                }
                err.emit();
            }

            MethodError::BadReturnType => {
                bug!("no return type expectations but got BadReturnType")
            }
        }
        None
    }

    /// Print out the type for use in value namespace.
    fn ty_to_value_string(&self, ty: Ty<'tcx>) -> String {
        match ty.kind {
            ty::Adt(def, substs) => format!("{}", ty::Instance::new(def.did, substs)),
            _ => self.ty_to_string(ty),
        }
    }

    fn suggest_use_candidates(&self,
                              err: &mut DiagnosticBuilder<'_>,
                              mut msg: String,
                              candidates: Vec<DefId>) {
        let module_did = self.tcx.hir().get_module_parent(self.body_id);
        let module_id = self.tcx.hir().as_local_hir_id(module_did).unwrap();
        let krate = self.tcx.hir().krate();
        let (span, found_use) = UsePlacementFinder::check(self.tcx, krate, module_id);
        if let Some(span) = span {
            let path_strings = candidates.iter().map(|did| {
                // Produce an additional newline to separate the new use statement
                // from the directly following item.
                let additional_newline = if found_use {
                    ""
                } else {
                    "\n"
                };
                format!(
                    "use {};\n{}",
                    with_crate_prefix(|| self.tcx.def_path_str(*did)),
                    additional_newline
                )
            });

            err.span_suggestions(span, &msg, path_strings, Applicability::MaybeIncorrect);
        } else {
            let limit = if candidates.len() == 5 { 5 } else { 4 };
            for (i, trait_did) in candidates.iter().take(limit).enumerate() {
                if candidates.len() > 1 {
                    msg.push_str(
                        &format!(
                            "\ncandidate #{}: `use {};`",
                            i + 1,
                            with_crate_prefix(|| self.tcx.def_path_str(*trait_did))
                        )
                    );
                } else {
                    msg.push_str(
                        &format!(
                            "\n`use {};`",
                            with_crate_prefix(|| self.tcx.def_path_str(*trait_did))
                        )
                    );
                }
            }
            if candidates.len() > limit {
                msg.push_str(&format!("\nand {} others", candidates.len() - limit));
            }
            err.note(&msg[..]);
        }
    }

    fn suggest_valid_traits(
        &self,
        err: &mut DiagnosticBuilder<'_>,
        valid_out_of_scope_traits: Vec<DefId>,
    ) -> bool {
        if !valid_out_of_scope_traits.is_empty() {
            let mut candidates = valid_out_of_scope_traits;
            candidates.sort();
            candidates.dedup();
            err.help("items from traits can only be used if the trait is in scope");
            let msg = format!(
                "the following {traits_are} implemented but not in scope; \
                 perhaps add a `use` for {one_of_them}:",
                traits_are = if candidates.len() == 1 {
                    "trait is"
                } else {
                    "traits are"
                },
                one_of_them = if candidates.len() == 1 {
                    "it"
                } else {
                    "one of them"
                },
            );

            self.suggest_use_candidates(err, msg, candidates);
            true
        } else {
            false
        }
    }

    fn suggest_traits_to_import<'b>(
        &self,
        err: &mut DiagnosticBuilder<'_>,
        span: Span,
        rcvr_ty: Ty<'tcx>,
        item_name: ast::Ident,
        source: SelfSource<'b>,
        valid_out_of_scope_traits: Vec<DefId>,
    ) {
        if self.suggest_valid_traits(err, valid_out_of_scope_traits) {
            return;
        }

        let type_is_local = self.type_derefs_to_local(span, rcvr_ty, source);

        // There are no traits implemented, so lets suggest some traits to
        // implement, by finding ones that have the item name, and are
        // legal to implement.
        let mut candidates = all_traits(self.tcx)
            .into_iter()
            .filter(|info| {
                // We approximate the coherence rules to only suggest
                // traits that are legal to implement by requiring that
                // either the type or trait is local. Multi-dispatch means
                // this isn't perfect (that is, there are cases when
                // implementing a trait would be legal but is rejected
                // here).
                (type_is_local || info.def_id.is_local()) &&
                    self.associated_item(info.def_id, item_name, Namespace::Value)
                        .filter(|item| {
                            // We only want to suggest public or local traits (#45781).
                            item.vis == ty::Visibility::Public || info.def_id.is_local()
                        })
                        .is_some()
            })
            .collect::<Vec<_>>();

        if !candidates.is_empty() {
            // Sort from most relevant to least relevant.
            candidates.sort_by(|a, b| a.cmp(b).reverse());
            candidates.dedup();

            let param_type = match rcvr_ty.kind {
                ty::Param(param) => Some(param),
                ty::Ref(_, ty, _) => match ty.kind {
                    ty::Param(param) => Some(param),
                    _ => None,
                }
                _ => None,
            };
            err.help(if param_type.is_some() {
                "items from traits can only be used if the type parameter is bounded by the trait"
            } else {
                "items from traits can only be used if the trait is implemented and in scope"
            });
            let message = |action| format!(
                "the following {traits_define} an item `{name}`, perhaps you need to {action} \
                 {one_of_them}:",
                traits_define = if candidates.len() == 1 {
                    "trait defines"
                } else {
                    "traits define"
                },
                action = action,
                one_of_them = if candidates.len() == 1 {
                    "it"
                } else {
                    "one of them"
                },
                name = item_name,
            );
            // Obtain the span for `param` and use it for a structured suggestion.
            let mut suggested = false;
            if let (Some(ref param), Some(ref table)) = (param_type, self.in_progress_tables) {
                let table = table.borrow();
                if let Some(did) = table.local_id_root {
                    let generics = self.tcx.generics_of(did);
                    let type_param = generics.type_param(param, self.tcx);
                    let hir = &self.tcx.hir();
                    if let Some(id) = hir.as_local_hir_id(type_param.def_id) {
                        // Get the `hir::Param` to verify whether it already has any bounds.
                        // We do this to avoid suggesting code that ends up as `T: FooBar`,
                        // instead we suggest `T: Foo + Bar` in that case.
                        match hir.get(id) {
                            Node::GenericParam(ref param) => {
                                let mut impl_trait = false;
                                let has_bounds = if let hir::GenericParamKind::Type {
                                    synthetic: Some(_), ..
                                } = &param.kind {
                                    // We've found `fn foo(x: impl Trait)` instead of
                                    // `fn foo<T>(x: T)`. We want to suggest the correct
                                    // `fn foo(x: impl Trait + TraitBound)` instead of
                                    // `fn foo<T: TraitBound>(x: T)`. (#63706)
                                    impl_trait = true;
                                    param.bounds.get(1)
                                } else {
                                    param.bounds.get(0)
                                };
                                let sp = hir.span(id);
                                let sp = if let Some(first_bound) = has_bounds {
                                    // `sp` only covers `T`, change it so that it covers
                                    // `T:` when appropriate
                                    sp.until(first_bound.span())
                                } else {
                                    sp
                                };
                                // FIXME: contrast `t.def_id` against `param.bounds` to not suggest
                                // traits already there. That can happen when the cause is that
                                // we're in a const scope or associated function used as a method.
                                err.span_suggestions(
                                    sp,
                                    &message(format!(
                                        "restrict type parameter `{}` with",
                                        param.name.ident(),
                                    )),
                                    candidates.iter().map(|t| format!(
                                        "{}{} {}{}",
                                        param.name.ident(),
                                        if impl_trait { " +" } else { ":" },
                                        self.tcx.def_path_str(t.def_id),
                                        if has_bounds.is_some() { " + "} else { "" },
                                    )),
                                    Applicability::MaybeIncorrect,
                                );
                                suggested = true;
                            }
                            Node::Item(hir::Item {
                                kind: hir::ItemKind::Trait(.., bounds, _), ident, ..
                            }) => {
                                let (sp, sep, article) = if bounds.is_empty() {
                                    (ident.span.shrink_to_hi(), ":", "a")
                                } else {
                                    (bounds.last().unwrap().span().shrink_to_hi(), " +", "another")
                                };
                                err.span_suggestions(
                                    sp,
                                    &message(format!("add {} supertrait for", article)),
                                    candidates.iter().map(|t| format!(
                                        "{} {}",
                                        sep,
                                        self.tcx.def_path_str(t.def_id),
                                    )),
                                    Applicability::MaybeIncorrect,
                                );
                                suggested = true;
                            }
                            _ => {}
                        }
                    }
                };
            }

            if !suggested {
                let mut msg = message(if let Some(param) = param_type {
                    format!("restrict type parameter `{}` with", param)
                } else {
                    "implement".to_string()
                });
                for (i, trait_info) in candidates.iter().enumerate() {
                    msg.push_str(&format!(
                        "\ncandidate #{}: `{}`",
                        i + 1,
                        self.tcx.def_path_str(trait_info.def_id),
                    ));
                }
                err.note(&msg[..]);
            }
        }
    }

    /// Checks whether there is a local type somewhere in the chain of
    /// autoderefs of `rcvr_ty`.
    fn type_derefs_to_local(&self,
                            span: Span,
                            rcvr_ty: Ty<'tcx>,
                            source: SelfSource<'_>) -> bool {
        fn is_local(ty: Ty<'_>) -> bool {
            match ty.kind {
                ty::Adt(def, _) => def.did.is_local(),
                ty::Foreign(did) => did.is_local(),

                ty::Dynamic(ref tr, ..) =>
                    tr.principal().map(|d| d.def_id().is_local()).unwrap_or(false),

                ty::Param(_) => true,

                // Everything else (primitive types, etc.) is effectively
                // non-local (there are "edge" cases, e.g., `(LocalType,)`, but
                // the noise from these sort of types is usually just really
                // annoying, rather than any sort of help).
                _ => false,
            }
        }

        // This occurs for UFCS desugaring of `T::method`, where there is no
        // receiver expression for the method call, and thus no autoderef.
        if let SelfSource::QPath(_) = source {
            return is_local(self.resolve_vars_with_obligations(rcvr_ty));
        }

        self.autoderef(span, rcvr_ty).any(|(ty, _)| is_local(ty))
    }
}

#[derive(Copy, Clone)]
pub enum SelfSource<'a> {
    QPath(&'a hir::Ty),
    MethodCall(&'a hir::Expr /* rcvr */),
}

#[derive(Copy, Clone)]
pub struct TraitInfo {
    pub def_id: DefId,
}

impl PartialEq for TraitInfo {
    fn eq(&self, other: &TraitInfo) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for TraitInfo {}
impl PartialOrd for TraitInfo {
    fn partial_cmp(&self, other: &TraitInfo) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TraitInfo {
    fn cmp(&self, other: &TraitInfo) -> Ordering {
        // Local crates are more important than remote ones (local:
        // `cnum == 0`), and otherwise we throw in the defid for totality.

        let lhs = (other.def_id.krate, other.def_id);
        let rhs = (self.def_id.krate, self.def_id);
        lhs.cmp(&rhs)
    }
}

/// Retrieves all traits in this crate and any dependent crates.
pub fn all_traits(tcx: TyCtxt<'_>) -> Vec<TraitInfo> {
    tcx.all_traits(LOCAL_CRATE).iter().map(|&def_id| TraitInfo { def_id }).collect()
}

/// Computes all traits in this crate and any dependent crates.
fn compute_all_traits(tcx: TyCtxt<'_>) -> Vec<DefId> {
    use hir::itemlikevisit;

    let mut traits = vec![];

    // Crate-local:

    struct Visitor<'a, 'tcx> {
        map: &'a hir_map::Map<'tcx>,
        traits: &'a mut Vec<DefId>,
    }

    impl<'v, 'a, 'tcx> itemlikevisit::ItemLikeVisitor<'v> for Visitor<'a, 'tcx> {
        fn visit_item(&mut self, i: &'v hir::Item) {
            match i.kind {
                hir::ItemKind::Trait(..) |
                hir::ItemKind::TraitAlias(..) => {
                    let def_id = self.map.local_def_id(i.hir_id);
                    self.traits.push(def_id);
                }
                _ => ()
            }
        }

        fn visit_trait_item(&mut self, _trait_item: &hir::TraitItem) {}

        fn visit_impl_item(&mut self, _impl_item: &hir::ImplItem) {}
    }

    tcx.hir().krate().visit_all_item_likes(&mut Visitor {
        map: &tcx.hir(),
        traits: &mut traits,
    });

    // Cross-crate:

    let mut external_mods = FxHashSet::default();
    fn handle_external_res(
        tcx: TyCtxt<'_>,
        traits: &mut Vec<DefId>,
        external_mods: &mut FxHashSet<DefId>,
        res: Res,
    ) {
        match res {
            Res::Def(DefKind::Trait, def_id) |
            Res::Def(DefKind::TraitAlias, def_id) => {
                traits.push(def_id);
            }
            Res::Def(DefKind::Mod, def_id) => {
                if !external_mods.insert(def_id) {
                    return;
                }
                for child in tcx.item_children(def_id).iter() {
                    handle_external_res(tcx, traits, external_mods, child.res)
                }
            }
            _ => {}
        }
    }
    for &cnum in tcx.crates().iter() {
        let def_id = DefId {
            krate: cnum,
            index: CRATE_DEF_INDEX,
        };
        handle_external_res(tcx, &mut traits, &mut external_mods, Res::Def(DefKind::Mod, def_id));
    }

    traits
}

pub fn provide(providers: &mut ty::query::Providers<'_>) {
    providers.all_traits = |tcx, cnum| {
        assert_eq!(cnum, LOCAL_CRATE);
        &tcx.arena.alloc(compute_all_traits(tcx))[..]
    }
}

struct UsePlacementFinder<'tcx> {
    target_module: hir::HirId,
    span: Option<Span>,
    found_use: bool,
    tcx: TyCtxt<'tcx>,
}

impl UsePlacementFinder<'tcx> {
    fn check(
        tcx: TyCtxt<'tcx>,
        krate: &'tcx hir::Crate,
        target_module: hir::HirId,
    ) -> (Option<Span>, bool) {
        let mut finder = UsePlacementFinder {
            target_module,
            span: None,
            found_use: false,
            tcx,
        };
        hir::intravisit::walk_crate(&mut finder, krate);
        (finder.span, finder.found_use)
    }
}

impl hir::intravisit::Visitor<'tcx> for UsePlacementFinder<'tcx> {
    fn visit_mod(
        &mut self,
        module: &'tcx hir::Mod,
        _: Span,
        hir_id: hir::HirId,
    ) {
        if self.span.is_some() {
            return;
        }
        if hir_id != self.target_module {
            hir::intravisit::walk_mod(self, module, hir_id);
            return;
        }
        // Find a `use` statement.
        for item_id in &module.item_ids {
            let item = self.tcx.hir().expect_item(item_id.id);
            match item.kind {
                hir::ItemKind::Use(..) => {
                    // Don't suggest placing a `use` before the prelude
                    // import or other generated ones.
                    if !item.span.from_expansion() {
                        self.span = Some(item.span.shrink_to_lo());
                        self.found_use = true;
                        return;
                    }
                },
                // Don't place `use` before `extern crate`...
                hir::ItemKind::ExternCrate(_) => {}
                // ...but do place them before the first other item.
                _ => if self.span.map_or(true, |span| item.span < span ) {
                    if !item.span.from_expansion() {
                        // Don't insert between attributes and an item.
                        if item.attrs.is_empty() {
                            self.span = Some(item.span.shrink_to_lo());
                        } else {
                            // Find the first attribute on the item.
                            for attr in &item.attrs {
                                if self.span.map_or(true, |span| attr.span < span) {
                                    self.span = Some(attr.span.shrink_to_lo());
                                }
                            }
                        }
                    }
                },
            }
        }
    }

    fn nested_visit_map<'this>(
        &'this mut self
    ) -> hir::intravisit::NestedVisitorMap<'this, 'tcx> {
        hir::intravisit::NestedVisitorMap::None
    }
}
