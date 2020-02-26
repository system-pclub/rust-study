//! # Lints in the Rust compiler
//!
//! This currently only contains the definitions and implementations
//! of most of the lints that `rustc` supports directly, it does not
//! contain the infrastructure for defining/registering lints. That is
//! available in `rustc::lint` and `rustc_driver::plugin` respectively.
//!
//! ## Note
//!
//! This API is completely unstable and subject to change.

#![doc(html_root_url = "https://doc.rust-lang.org/nightly/")]

#![cfg_attr(test, feature(test))]
#![feature(box_patterns)]
#![feature(box_syntax)]
#![feature(nll)]
#![feature(matches_macro)]

#![recursion_limit="256"]

#[macro_use]
extern crate rustc;

mod array_into_iter;
mod nonstandard_style;
mod redundant_semicolon;
pub mod builtin;
mod types;
mod unused;
mod non_ascii_idents;

use rustc::lint;
use rustc::lint::{EarlyContext, LateContext, LateLintPass, EarlyLintPass, LintPass, LintArray};
use rustc::lint::builtin::{
    BARE_TRAIT_OBJECTS,
    ELIDED_LIFETIMES_IN_PATHS,
    EXPLICIT_OUTLIVES_REQUIREMENTS,
    INTRA_DOC_LINK_RESOLUTION_FAILURE,
    MISSING_DOC_CODE_EXAMPLES,
    PRIVATE_DOC_TESTS,
};
use rustc::hir;
use rustc::hir::def_id::DefId;
use rustc::ty::query::Providers;
use rustc::ty::TyCtxt;

use syntax::ast;
use syntax_pos::Span;

use lint::LintId;

use redundant_semicolon::*;
use nonstandard_style::*;
use builtin::*;
use types::*;
use unused::*;
use non_ascii_idents::*;
use rustc::lint::internal::*;
use array_into_iter::ArrayIntoIter;

/// Useful for other parts of the compiler.
pub use builtin::SoftLints;

pub fn provide(providers: &mut Providers<'_>) {
    *providers = Providers {
        lint_mod,
        ..*providers
    };
}

fn lint_mod(tcx: TyCtxt<'_>, module_def_id: DefId) {
    lint::late_lint_mod(tcx, module_def_id, BuiltinCombinedModuleLateLintPass::new());
}

macro_rules! pre_expansion_lint_passes {
    ($macro:path, $args:tt) => (
        $macro!($args, [
            KeywordIdents: KeywordIdents,
            UnusedDocComment: UnusedDocComment,
        ]);
    )
}

macro_rules! early_lint_passes {
    ($macro:path, $args:tt) => (
        $macro!($args, [
            UnusedParens: UnusedParens,
            UnusedImportBraces: UnusedImportBraces,
            UnsafeCode: UnsafeCode,
            AnonymousParameters: AnonymousParameters,
            EllipsisInclusiveRangePatterns: EllipsisInclusiveRangePatterns::default(),
            NonCamelCaseTypes: NonCamelCaseTypes,
            DeprecatedAttr: DeprecatedAttr::new(),
            WhileTrue: WhileTrue,
            NonAsciiIdents: NonAsciiIdents,
            IncompleteFeatures: IncompleteFeatures,
            RedundantSemicolon: RedundantSemicolon,
        ]);
    )
}

macro_rules! declare_combined_early_pass {
    ([$name:ident], $passes:tt) => (
        early_lint_methods!(declare_combined_early_lint_pass, [pub $name, $passes]);
    )
}

pre_expansion_lint_passes!(declare_combined_early_pass, [BuiltinCombinedPreExpansionLintPass]);
early_lint_passes!(declare_combined_early_pass, [BuiltinCombinedEarlyLintPass]);

macro_rules! late_lint_passes {
    ($macro:path, $args:tt) => (
        $macro!($args, [
            // FIXME: Look into regression when this is used as a module lint
            // May Depend on constants elsewhere
            UnusedBrokenConst: UnusedBrokenConst,

            // Uses attr::is_used which is untracked, can't be an incremental module pass.
            UnusedAttributes: UnusedAttributes::new(),

            // Needs to run after UnusedAttributes as it marks all `feature` attributes as used.
            UnstableFeatures: UnstableFeatures,

            // Tracks state across modules
            UnnameableTestItems: UnnameableTestItems::new(),

            // Tracks attributes of parents
            MissingDoc: MissingDoc::new(),

            // Depends on access levels
            // FIXME: Turn the computation of types which implement Debug into a query
            // and change this to a module lint pass
            MissingDebugImplementations: MissingDebugImplementations::default(),

            ArrayIntoIter: ArrayIntoIter,
        ]);
    )
}

macro_rules! late_lint_mod_passes {
    ($macro:path, $args:tt) => (
        $macro!($args, [
            HardwiredLints: HardwiredLints,
            ImproperCTypes: ImproperCTypes,
            VariantSizeDifferences: VariantSizeDifferences,
            BoxPointers: BoxPointers,
            PathStatements: PathStatements,

            // Depends on referenced function signatures in expressions
            UnusedResults: UnusedResults,

            NonUpperCaseGlobals: NonUpperCaseGlobals,
            NonShorthandFieldPatterns: NonShorthandFieldPatterns,
            UnusedAllocation: UnusedAllocation,

            // Depends on types used in type definitions
            MissingCopyImplementations: MissingCopyImplementations,

            PluginAsLibrary: PluginAsLibrary,

            // Depends on referenced function signatures in expressions
            MutableTransmutes: MutableTransmutes,

            TypeAliasBounds: TypeAliasBounds,

            TrivialConstraints: TrivialConstraints,
            TypeLimits: TypeLimits::new(),

            NonSnakeCase: NonSnakeCase,
            InvalidNoMangleItems: InvalidNoMangleItems,

            // Depends on access levels
            UnreachablePub: UnreachablePub,

            ExplicitOutlivesRequirements: ExplicitOutlivesRequirements,
            InvalidValue: InvalidValue,
        ]);
    )
}

macro_rules! declare_combined_late_pass {
    ([$v:vis $name:ident], $passes:tt) => (
        late_lint_methods!(declare_combined_late_lint_pass, [$v $name, $passes], ['tcx]);
    )
}

// FIXME: Make a separate lint type which do not require typeck tables
late_lint_passes!(declare_combined_late_pass, [pub BuiltinCombinedLateLintPass]);

late_lint_mod_passes!(declare_combined_late_pass, [BuiltinCombinedModuleLateLintPass]);

pub fn new_lint_store(no_interleave_lints: bool, internal_lints: bool) -> lint::LintStore {
    let mut lint_store = lint::LintStore::new();

    register_builtins(&mut lint_store, no_interleave_lints);
    if internal_lints {
        register_internals(&mut lint_store);
    }

    lint_store
}

/// Tell the `LintStore` about all the built-in lints (the ones
/// defined in this crate and the ones defined in
/// `rustc::lint::builtin`).
fn register_builtins(store: &mut lint::LintStore, no_interleave_lints: bool) {
    macro_rules! add_lint_group {
        ($name:expr, $($lint:ident),*) => (
            store.register_group(false, $name, None, vec![$(LintId::of($lint)),*]);
        )
    }

    macro_rules! register_pass {
        ($method:ident, $ty:ident, $constructor:expr) => (
            store.register_lints(&$ty::get_lints());
            store.$method(|| box $constructor);
        )
    }

    macro_rules! register_passes {
        ($method:ident, [$($passes:ident: $constructor:expr,)*]) => (
            $(
                register_pass!($method, $passes, $constructor);
            )*
        )
    }

    if no_interleave_lints {
        pre_expansion_lint_passes!(register_passes, register_pre_expansion_pass);
        early_lint_passes!(register_passes, register_early_pass);
        late_lint_passes!(register_passes, register_late_pass);
        late_lint_mod_passes!(register_passes, register_late_mod_pass);
    } else {
        store.register_lints(&BuiltinCombinedPreExpansionLintPass::get_lints());
        store.register_lints(&BuiltinCombinedEarlyLintPass::get_lints());
        store.register_lints(&BuiltinCombinedModuleLateLintPass::get_lints());
        store.register_lints(&BuiltinCombinedLateLintPass::get_lints());
    }

    add_lint_group!("nonstandard_style",
                    NON_CAMEL_CASE_TYPES,
                    NON_SNAKE_CASE,
                    NON_UPPER_CASE_GLOBALS);

    add_lint_group!("unused",
                    UNUSED_IMPORTS,
                    UNUSED_VARIABLES,
                    UNUSED_ASSIGNMENTS,
                    DEAD_CODE,
                    UNUSED_MUT,
                    UNREACHABLE_CODE,
                    UNREACHABLE_PATTERNS,
                    OVERLAPPING_PATTERNS,
                    UNUSED_MUST_USE,
                    UNUSED_UNSAFE,
                    PATH_STATEMENTS,
                    UNUSED_ATTRIBUTES,
                    UNUSED_MACROS,
                    UNUSED_ALLOCATION,
                    UNUSED_DOC_COMMENTS,
                    UNUSED_EXTERN_CRATES,
                    UNUSED_FEATURES,
                    UNUSED_LABELS,
                    UNUSED_PARENS);

    add_lint_group!("rust_2018_idioms",
                    BARE_TRAIT_OBJECTS,
                    UNUSED_EXTERN_CRATES,
                    ELLIPSIS_INCLUSIVE_RANGE_PATTERNS,
                    ELIDED_LIFETIMES_IN_PATHS,
                    EXPLICIT_OUTLIVES_REQUIREMENTS

                    // FIXME(#52665, #47816) not always applicable and not all
                    // macros are ready for this yet.
                    // UNREACHABLE_PUB,

                    // FIXME macro crates are not up for this yet, too much
                    // breakage is seen if we try to encourage this lint.
                    // MACRO_USE_EXTERN_CRATE,
                    );

    add_lint_group!("rustdoc",
                    INTRA_DOC_LINK_RESOLUTION_FAILURE,
                    MISSING_DOC_CODE_EXAMPLES,
                    PRIVATE_DOC_TESTS);

    // Register renamed and removed lints.
    store.register_renamed("single_use_lifetime", "single_use_lifetimes");
    store.register_renamed("elided_lifetime_in_path", "elided_lifetimes_in_paths");
    store.register_renamed("bare_trait_object", "bare_trait_objects");
    store.register_renamed("unstable_name_collision", "unstable_name_collisions");
    store.register_renamed("unused_doc_comment", "unused_doc_comments");
    store.register_renamed("async_idents", "keyword_idents");
    store.register_removed("unknown_features", "replaced by an error");
    store.register_removed("unsigned_negation", "replaced by negate_unsigned feature gate");
    store.register_removed("negate_unsigned", "cast a signed value instead");
    store.register_removed("raw_pointer_derive", "using derive with raw pointers is ok");
    // Register lint group aliases.
    store.register_group_alias("nonstandard_style", "bad_style");
    // This was renamed to `raw_pointer_derive`, which was then removed,
    // so it is also considered removed.
    store.register_removed("raw_pointer_deriving", "using derive with raw pointers is ok");
    store.register_removed("drop_with_repr_extern", "drop flags have been removed");
    store.register_removed("fat_ptr_transmutes", "was accidentally removed back in 2014");
    store.register_removed("deprecated_attr", "use `deprecated` instead");
    store.register_removed("transmute_from_fn_item_types",
        "always cast functions before transmuting them");
    store.register_removed("hr_lifetime_in_assoc_type",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/33685");
    store.register_removed("inaccessible_extern_crate",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36886");
    store.register_removed("super_or_self_in_global_path",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36888");
    store.register_removed("overlapping_inherent_impls",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36889");
    store.register_removed("illegal_floating_point_constant_pattern",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36890");
    store.register_removed("illegal_struct_or_enum_constant_pattern",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36891");
    store.register_removed("lifetime_underscore",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36892");
    store.register_removed("extra_requirement_in_impl",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/37166");
    store.register_removed("legacy_imports",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/38260");
    store.register_removed("coerce_never",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/48950");
    store.register_removed("resolve_trait_on_defaulted_unit",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/48950");
    store.register_removed("private_no_mangle_fns",
        "no longer a warning, `#[no_mangle]` functions always exported");
    store.register_removed("private_no_mangle_statics",
        "no longer a warning, `#[no_mangle]` statics always exported");
    store.register_removed("bad_repr",
        "replaced with a generic attribute input check");
    store.register_removed("duplicate_matcher_binding_name",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/57742");
    store.register_removed("incoherent_fundamental_impls",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/46205");
    store.register_removed("legacy_constructor_visibility",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/39207");
    store.register_removed("legacy_disrectory_ownership",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/37872");
    store.register_removed("safe_extern_statics",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/36247");
    store.register_removed("parenthesized_params_in_types_and_modules",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/42238");
    store.register_removed("duplicate_macro_exports",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/35896");
    store.register_removed("nested_impl_trait",
        "converted into hard error, see https://github.com/rust-lang/rust/issues/59014");
}

fn register_internals(store: &mut lint::LintStore) {
    store.register_lints(&DefaultHashTypes::get_lints());
    store.register_early_pass(|| box DefaultHashTypes::new());
    store.register_lints(&LintPassImpl::get_lints());
    store.register_early_pass(|| box LintPassImpl);
    store.register_lints(&TyTyKind::get_lints());
    store.register_late_pass(|| box TyTyKind);
    store.register_group(
        false,
        "rustc::internal",
        None,
        vec![
            LintId::of(DEFAULT_HASH_TYPES),
            LintId::of(USAGE_OF_TY_TYKIND),
            LintId::of(LINT_PASS_IMPL_WITHOUT_MACRO),
            LintId::of(TY_PASS_BY_REFERENCE),
            LintId::of(USAGE_OF_QUALIFIED_TY),
        ],
    );
}
