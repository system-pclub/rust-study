//! Lints, aka compiler warnings.
//!
//! A 'lint' check is a kind of miscellaneous constraint that a user _might_
//! want to enforce, but might reasonably want to permit as well, on a
//! module-by-module basis. They contrast with static constraints enforced by
//! other phases of the compiler, which are generally required to hold in order
//! to compile the program at all.
//!
//! Most lints can be written as `LintPass` instances. These run after
//! all other analyses. The `LintPass`es built into rustc are defined
//! within `builtin.rs`, which has further comments on how to add such a lint.
//! rustc can also load user-defined lint plugins via the plugin mechanism.
//!
//! Some of rustc's lints are defined elsewhere in the compiler and work by
//! calling `add_lint()` on the overall `Session` object. This works when
//! it happens before the main lint pass, which emits the lints stored by
//! `add_lint()`. To emit lints after the main lint pass (from codegen, for
//! example) requires more effort. See `emit_lint` and `GatherNodeLevels`
//! in `context.rs`.

pub use self::Level::*;
pub use self::LintSource::*;

use rustc_data_structures::sync;

use crate::hir::def_id::{CrateNum, LOCAL_CRATE};
use crate::hir::intravisit;
use crate::hir;
use crate::lint::builtin::BuiltinLintDiagnostics;
use crate::lint::builtin::parser::{ILL_FORMED_ATTRIBUTE_INPUT, META_VARIABLE_MISUSE};
use crate::lint::builtin::parser::INCOMPLETE_INCLUDE;
use crate::session::{Session, DiagnosticMessageId};
use crate::ty::TyCtxt;
use crate::ty::query::Providers;
use crate::util::nodemap::NodeMap;
use errors::{DiagnosticBuilder, DiagnosticId};
use std::{hash, ptr};
use syntax::ast;
use syntax::source_map::{MultiSpan, ExpnKind, DesugaringKind};
use syntax::early_buffered_lints::BufferedEarlyLintId;
use syntax::edition::Edition;
use syntax::symbol::{Symbol, sym};
use syntax_pos::hygiene::MacroKind;
use syntax_pos::Span;

pub use crate::lint::context::{LateContext, EarlyContext, LintContext, LintStore,
                        check_crate, check_ast_crate, late_lint_mod, CheckLintNameResult,
                        BufferedEarlyLint,};

/// Specification of a single lint.
#[derive(Copy, Clone, Debug)]
pub struct Lint {
    /// A string identifier for the lint.
    ///
    /// This identifies the lint in attributes and in command-line arguments.
    /// In those contexts it is always lowercase, but this field is compared
    /// in a way which is case-insensitive for ASCII characters. This allows
    /// `declare_lint!()` invocations to follow the convention of upper-case
    /// statics without repeating the name.
    ///
    /// The name is written with underscores, e.g., "unused_imports".
    /// On the command line, underscores become dashes.
    pub name: &'static str,

    /// Default level for the lint.
    pub default_level: Level,

    /// Description of the lint or the issue it detects.
    ///
    /// e.g., "imports that are never used"
    pub desc: &'static str,

    /// Starting at the given edition, default to the given lint level. If this is `None`, then use
    /// `default_level`.
    pub edition_lint_opts: Option<(Edition, Level)>,

    /// `true` if this lint is reported even inside expansions of external macros.
    pub report_in_external_macro: bool,

    pub future_incompatible: Option<FutureIncompatibleInfo>,

    pub is_plugin: bool,
}

/// Extra information for a future incompatibility lint.
#[derive(Copy, Clone, Debug)]
pub struct FutureIncompatibleInfo {
    /// e.g., a URL for an issue/PR/RFC or error code
    pub reference: &'static str,
    /// If this is an edition fixing lint, the edition in which
    /// this lint becomes obsolete
    pub edition: Option<Edition>,
}

impl Lint {
    pub const fn default_fields_for_macro() -> Self {
        Lint {
            name: "",
            default_level: Level::Forbid,
            desc: "",
            edition_lint_opts: None,
            is_plugin: false,
            report_in_external_macro: false,
            future_incompatible: None,
        }
    }

    /// Returns the `rust::lint::Lint` for a `syntax::early_buffered_lints::BufferedEarlyLintId`.
    pub fn from_parser_lint_id(lint_id: BufferedEarlyLintId) -> &'static Self {
        match lint_id {
            BufferedEarlyLintId::IllFormedAttributeInput => ILL_FORMED_ATTRIBUTE_INPUT,
            BufferedEarlyLintId::MetaVariableMisuse => META_VARIABLE_MISUSE,
            BufferedEarlyLintId::IncompleteInclude => INCOMPLETE_INCLUDE,
        }
    }

    /// Gets the lint's name, with ASCII letters converted to lowercase.
    pub fn name_lower(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    pub fn default_level(&self, session: &Session) -> Level {
        self.edition_lint_opts
            .filter(|(e, _)| *e <= session.edition())
            .map(|(_, l)| l)
            .unwrap_or(self.default_level)
    }
}

/// Declares a static item of type `&'static Lint`.
#[macro_export]
macro_rules! declare_lint {
    ($vis: vis $NAME: ident, $Level: ident, $desc: expr) => (
        declare_lint!(
            $vis $NAME, $Level, $desc,
        );
    );
    ($vis: vis $NAME: ident, $Level: ident, $desc: expr,
     $(@future_incompatible = $fi:expr;)? $($v:ident),*) => (
        $vis static $NAME: &$crate::lint::Lint = &$crate::lint::Lint {
            name: stringify!($NAME),
            default_level: $crate::lint::$Level,
            desc: $desc,
            edition_lint_opts: None,
            is_plugin: false,
            $($v: true,)*
            $(future_incompatible: Some($fi),)*
            ..$crate::lint::Lint::default_fields_for_macro()
        };
    );
    ($vis: vis $NAME: ident, $Level: ident, $desc: expr,
     $lint_edition: expr => $edition_level: ident
    ) => (
        $vis static $NAME: &$crate::lint::Lint = &$crate::lint::Lint {
            name: stringify!($NAME),
            default_level: $crate::lint::$Level,
            desc: $desc,
            edition_lint_opts: Some(($lint_edition, $crate::lint::Level::$edition_level)),
            report_in_external_macro: false,
            is_plugin: false,
        };
    );
}

#[macro_export]
macro_rules! declare_tool_lint {
    (
        $(#[$attr:meta])* $vis:vis $tool:ident ::$NAME:ident, $Level: ident, $desc: expr
    ) => (
        declare_tool_lint!{$(#[$attr])* $vis $tool::$NAME, $Level, $desc, false}
    );
    (
        $(#[$attr:meta])* $vis:vis $tool:ident ::$NAME:ident, $Level:ident, $desc:expr,
        report_in_external_macro: $rep:expr
    ) => (
         declare_tool_lint!{$(#[$attr])* $vis $tool::$NAME, $Level, $desc, $rep}
    );
    (
        $(#[$attr:meta])* $vis:vis $tool:ident ::$NAME:ident, $Level:ident, $desc:expr,
        $external:expr
    ) => (
        $(#[$attr])*
        $vis static $NAME: &$crate::lint::Lint = &$crate::lint::Lint {
            name: &concat!(stringify!($tool), "::", stringify!($NAME)),
            default_level: $crate::lint::$Level,
            desc: $desc,
            edition_lint_opts: None,
            report_in_external_macro: $external,
            future_incompatible: None,
            is_plugin: true,
        };
    );
}

/// Declares a static `LintArray` and return it as an expression.
#[macro_export]
macro_rules! lint_array {
    ($( $lint:expr ),* ,) => { lint_array!( $($lint),* ) };
    ($( $lint:expr ),*) => {{
        vec![$($lint),*]
    }}
}

pub type LintArray = Vec<&'static Lint>;

pub trait LintPass {
    fn name(&self) -> &'static str;
}

/// Implements `LintPass for $name` with the given list of `Lint` statics.
#[macro_export]
macro_rules! impl_lint_pass {
    ($name:ident => [$($lint:expr),* $(,)?]) => {
        impl LintPass for $name {
            fn name(&self) -> &'static str { stringify!($name) }
        }
        impl $name {
            pub fn get_lints() -> LintArray { $crate::lint_array!($($lint),*) }
        }
    };
}

/// Declares a type named `$name` which implements `LintPass`.
/// To the right of `=>` a comma separated list of `Lint` statics is given.
#[macro_export]
macro_rules! declare_lint_pass {
    ($(#[$m:meta])* $name:ident => [$($lint:expr),* $(,)?]) => {
        $(#[$m])* #[derive(Copy, Clone)] pub struct $name;
        $crate::impl_lint_pass!($name => [$($lint),*]);
    };
}

#[macro_export]
macro_rules! late_lint_methods {
    ($macro:path, $args:tt, [$hir:tt]) => (
        $macro!($args, [$hir], [
            fn check_param(a: &$hir hir::Param);
            fn check_body(a: &$hir hir::Body);
            fn check_body_post(a: &$hir hir::Body);
            fn check_name(a: Span, b: ast::Name);
            fn check_crate(a: &$hir hir::Crate);
            fn check_crate_post(a: &$hir hir::Crate);
            fn check_mod(a: &$hir hir::Mod, b: Span, c: hir::HirId);
            fn check_mod_post(a: &$hir hir::Mod, b: Span, c: hir::HirId);
            fn check_foreign_item(a: &$hir hir::ForeignItem);
            fn check_foreign_item_post(a: &$hir hir::ForeignItem);
            fn check_item(a: &$hir hir::Item);
            fn check_item_post(a: &$hir hir::Item);
            fn check_local(a: &$hir hir::Local);
            fn check_block(a: &$hir hir::Block);
            fn check_block_post(a: &$hir hir::Block);
            fn check_stmt(a: &$hir hir::Stmt);
            fn check_arm(a: &$hir hir::Arm);
            fn check_pat(a: &$hir hir::Pat);
            fn check_expr(a: &$hir hir::Expr);
            fn check_expr_post(a: &$hir hir::Expr);
            fn check_ty(a: &$hir hir::Ty);
            fn check_generic_param(a: &$hir hir::GenericParam);
            fn check_generics(a: &$hir hir::Generics);
            fn check_where_predicate(a: &$hir hir::WherePredicate);
            fn check_poly_trait_ref(a: &$hir hir::PolyTraitRef, b: hir::TraitBoundModifier);
            fn check_fn(
                a: hir::intravisit::FnKind<$hir>,
                b: &$hir hir::FnDecl,
                c: &$hir hir::Body,
                d: Span,
                e: hir::HirId);
            fn check_fn_post(
                a: hir::intravisit::FnKind<$hir>,
                b: &$hir hir::FnDecl,
                c: &$hir hir::Body,
                d: Span,
                e: hir::HirId
            );
            fn check_trait_item(a: &$hir hir::TraitItem);
            fn check_trait_item_post(a: &$hir hir::TraitItem);
            fn check_impl_item(a: &$hir hir::ImplItem);
            fn check_impl_item_post(a: &$hir hir::ImplItem);
            fn check_struct_def(a: &$hir hir::VariantData);
            fn check_struct_def_post(a: &$hir hir::VariantData);
            fn check_struct_field(a: &$hir hir::StructField);
            fn check_variant(a: &$hir hir::Variant);
            fn check_variant_post(a: &$hir hir::Variant);
            fn check_lifetime(a: &$hir hir::Lifetime);
            fn check_path(a: &$hir hir::Path, b: hir::HirId);
            fn check_attribute(a: &$hir ast::Attribute);

            /// Called when entering a syntax node that can have lint attributes such
            /// as `#[allow(...)]`. Called with *all* the attributes of that node.
            fn enter_lint_attrs(a: &$hir [ast::Attribute]);

            /// Counterpart to `enter_lint_attrs`.
            fn exit_lint_attrs(a: &$hir [ast::Attribute]);
        ]);
    )
}

/// Trait for types providing lint checks.
///
/// Each `check` method checks a single syntax node, and should not
/// invoke methods recursively (unlike `Visitor`). By default they
/// do nothing.
//
// FIXME: eliminate the duplication with `Visitor`. But this also
// contains a few lint-specific methods with no equivalent in `Visitor`.

macro_rules! expand_lint_pass_methods {
    ($context:ty, [$($(#[$attr:meta])* fn $name:ident($($param:ident: $arg:ty),*);)*]) => (
        $(#[inline(always)] fn $name(&mut self, _: $context, $(_: $arg),*) {})*
    )
}

macro_rules! declare_late_lint_pass {
    ([], [$hir:tt], [$($methods:tt)*]) => (
        pub trait LateLintPass<'a, $hir>: LintPass {
            expand_lint_pass_methods!(&LateContext<'a, $hir>, [$($methods)*]);
        }
    )
}

late_lint_methods!(declare_late_lint_pass, [], ['tcx]);

#[macro_export]
macro_rules! expand_combined_late_lint_pass_method {
    ([$($passes:ident),*], $self: ident, $name: ident, $params:tt) => ({
        $($self.$passes.$name $params;)*
    })
}

#[macro_export]
macro_rules! expand_combined_late_lint_pass_methods {
    ($passes:tt, [$($(#[$attr:meta])* fn $name:ident($($param:ident: $arg:ty),*);)*]) => (
        $(fn $name(&mut self, context: &LateContext<'a, 'tcx>, $($param: $arg),*) {
            expand_combined_late_lint_pass_method!($passes, self, $name, (context, $($param),*));
        })*
    )
}

#[macro_export]
macro_rules! declare_combined_late_lint_pass {
    ([$v:vis $name:ident, [$($passes:ident: $constructor:expr,)*]], [$hir:tt], $methods:tt) => (
        #[allow(non_snake_case)]
        $v struct $name {
            $($passes: $passes,)*
        }

        impl $name {
            $v fn new() -> Self {
                Self {
                    $($passes: $constructor,)*
                }
            }

            $v fn get_lints() -> LintArray {
                let mut lints = Vec::new();
                $(lints.extend_from_slice(&$passes::get_lints());)*
                lints
            }
        }

        impl<'a, 'tcx> LateLintPass<'a, 'tcx> for $name {
            expand_combined_late_lint_pass_methods!([$($passes),*], $methods);
        }

        impl LintPass for $name {
            fn name(&self) -> &'static str {
                panic!()
            }
        }
    )
}

#[macro_export]
macro_rules! early_lint_methods {
    ($macro:path, $args:tt) => (
        $macro!($args, [
            fn check_param(a: &ast::Param);
            fn check_ident(a: ast::Ident);
            fn check_crate(a: &ast::Crate);
            fn check_crate_post(a: &ast::Crate);
            fn check_mod(a: &ast::Mod, b: Span, c: ast::NodeId);
            fn check_mod_post(a: &ast::Mod, b: Span, c: ast::NodeId);
            fn check_foreign_item(a: &ast::ForeignItem);
            fn check_foreign_item_post(a: &ast::ForeignItem);
            fn check_item(a: &ast::Item);
            fn check_item_post(a: &ast::Item);
            fn check_local(a: &ast::Local);
            fn check_block(a: &ast::Block);
            fn check_block_post(a: &ast::Block);
            fn check_stmt(a: &ast::Stmt);
            fn check_arm(a: &ast::Arm);
            fn check_pat(a: &ast::Pat);
            fn check_pat_post(a: &ast::Pat);
            fn check_expr(a: &ast::Expr);
            fn check_expr_post(a: &ast::Expr);
            fn check_ty(a: &ast::Ty);
            fn check_generic_param(a: &ast::GenericParam);
            fn check_generics(a: &ast::Generics);
            fn check_where_predicate(a: &ast::WherePredicate);
            fn check_poly_trait_ref(a: &ast::PolyTraitRef,
                                    b: &ast::TraitBoundModifier);
            fn check_fn(a: syntax::visit::FnKind<'_>, b: &ast::FnDecl, c: Span, d_: ast::NodeId);
            fn check_fn_post(
                a: syntax::visit::FnKind<'_>,
                b: &ast::FnDecl,
                c: Span,
                d: ast::NodeId
            );
            fn check_trait_item(a: &ast::TraitItem);
            fn check_trait_item_post(a: &ast::TraitItem);
            fn check_impl_item(a: &ast::ImplItem);
            fn check_impl_item_post(a: &ast::ImplItem);
            fn check_struct_def(a: &ast::VariantData);
            fn check_struct_def_post(a: &ast::VariantData);
            fn check_struct_field(a: &ast::StructField);
            fn check_variant(a: &ast::Variant);
            fn check_variant_post(a: &ast::Variant);
            fn check_lifetime(a: &ast::Lifetime);
            fn check_path(a: &ast::Path, b: ast::NodeId);
            fn check_attribute(a: &ast::Attribute);
            fn check_mac_def(a: &ast::MacroDef, b: ast::NodeId);
            fn check_mac(a: &ast::Mac);

            /// Called when entering a syntax node that can have lint attributes such
            /// as `#[allow(...)]`. Called with *all* the attributes of that node.
            fn enter_lint_attrs(a: &[ast::Attribute]);

            /// Counterpart to `enter_lint_attrs`.
            fn exit_lint_attrs(a: &[ast::Attribute]);
        ]);
    )
}

macro_rules! expand_early_lint_pass_methods {
    ($context:ty, [$($(#[$attr:meta])* fn $name:ident($($param:ident: $arg:ty),*);)*]) => (
        $(#[inline(always)] fn $name(&mut self, _: $context, $(_: $arg),*) {})*
    )
}

macro_rules! declare_early_lint_pass {
    ([], [$($methods:tt)*]) => (
        pub trait EarlyLintPass: LintPass {
            expand_early_lint_pass_methods!(&EarlyContext<'_>, [$($methods)*]);
        }
    )
}

early_lint_methods!(declare_early_lint_pass, []);

#[macro_export]
macro_rules! expand_combined_early_lint_pass_method {
    ([$($passes:ident),*], $self: ident, $name: ident, $params:tt) => ({
        $($self.$passes.$name $params;)*
    })
}

#[macro_export]
macro_rules! expand_combined_early_lint_pass_methods {
    ($passes:tt, [$($(#[$attr:meta])* fn $name:ident($($param:ident: $arg:ty),*);)*]) => (
        $(fn $name(&mut self, context: &EarlyContext<'_>, $($param: $arg),*) {
            expand_combined_early_lint_pass_method!($passes, self, $name, (context, $($param),*));
        })*
    )
}

#[macro_export]
macro_rules! declare_combined_early_lint_pass {
    ([$v:vis $name:ident, [$($passes:ident: $constructor:expr,)*]], $methods:tt) => (
        #[allow(non_snake_case)]
        $v struct $name {
            $($passes: $passes,)*
        }

        impl $name {
            $v fn new() -> Self {
                Self {
                    $($passes: $constructor,)*
                }
            }

            $v fn get_lints() -> LintArray {
                let mut lints = Vec::new();
                $(lints.extend_from_slice(&$passes::get_lints());)*
                lints
            }
        }

        impl EarlyLintPass for $name {
            expand_combined_early_lint_pass_methods!([$($passes),*], $methods);
        }

        impl LintPass for $name {
            fn name(&self) -> &'static str {
                panic!()
            }
        }
    )
}

/// A lint pass boxed up as a trait object.
pub type EarlyLintPassObject = Box<dyn EarlyLintPass + sync::Send + sync::Sync + 'static>;
pub type LateLintPassObject = Box<dyn for<'a, 'tcx> LateLintPass<'a, 'tcx> + sync::Send
                                                                           + sync::Sync + 'static>;

/// Identifies a lint known to the compiler.
#[derive(Clone, Copy, Debug)]
pub struct LintId {
    // Identity is based on pointer equality of this field.
    lint: &'static Lint,
}

impl PartialEq for LintId {
    fn eq(&self, other: &LintId) -> bool {
        ptr::eq(self.lint, other.lint)
    }
}

impl Eq for LintId { }

impl hash::Hash for LintId {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        let ptr = self.lint as *const Lint;
        ptr.hash(state);
    }
}

impl LintId {
    /// Gets the `LintId` for a `Lint`.
    pub fn of(lint: &'static Lint) -> LintId {
        LintId {
            lint,
        }
    }

    pub fn lint_name_raw(&self) -> &'static str {
        self.lint.name
    }

    /// Gets the name of the lint.
    pub fn to_string(&self) -> String {
        self.lint.name_lower()
    }
}

/// Setting for how to handle a lint.
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Debug, Hash, HashStable)]
pub enum Level {
    Allow, Warn, Deny, Forbid,
}

impl Level {
    /// Converts a level to a lower-case string.
    pub fn as_str(self) -> &'static str {
        match self {
            Allow => "allow",
            Warn => "warn",
            Deny => "deny",
            Forbid => "forbid",
        }
    }

    /// Converts a lower-case string to a level.
    pub fn from_str(x: &str) -> Option<Level> {
        match x {
            "allow" => Some(Allow),
            "warn" => Some(Warn),
            "deny" => Some(Deny),
            "forbid" => Some(Forbid),
            _ => None,
        }
    }

    /// Converts a symbol to a level.
    pub fn from_symbol(x: Symbol) -> Option<Level> {
        match x {
            sym::allow => Some(Allow),
            sym::warn => Some(Warn),
            sym::deny => Some(Deny),
            sym::forbid => Some(Forbid),
            _ => None,
        }
    }
}

/// How a lint level was set.
#[derive(Clone, Copy, PartialEq, Eq, HashStable)]
pub enum LintSource {
    /// Lint is at the default level as declared
    /// in rustc or a plugin.
    Default,

    /// Lint level was set by an attribute.
    Node(ast::Name, Span, Option<Symbol> /* RFC 2383 reason */),

    /// Lint level was set by a command-line flag.
    CommandLine(Symbol),
}

pub type LevelSource = (Level, LintSource);

pub mod builtin;
pub mod internal;
mod context;
mod levels;

pub use self::levels::{LintLevelSets, LintLevelMap};

#[derive(Default)]
pub struct LintBuffer {
    map: NodeMap<Vec<BufferedEarlyLint>>,
}

impl LintBuffer {
    pub fn add_lint(&mut self,
                    lint: &'static Lint,
                    id: ast::NodeId,
                    sp: MultiSpan,
                    msg: &str,
                    diagnostic: BuiltinLintDiagnostics) {
        let early_lint = BufferedEarlyLint {
            lint_id: LintId::of(lint),
            ast_id: id,
            span: sp,
            msg: msg.to_string(),
            diagnostic
        };
        let arr = self.map.entry(id).or_default();
        if !arr.contains(&early_lint) {
            arr.push(early_lint);
        }
    }

    fn take(&mut self, id: ast::NodeId) -> Vec<BufferedEarlyLint> {
        self.map.remove(&id).unwrap_or_default()
    }

    pub fn buffer_lint<S: Into<MultiSpan>>(
        &mut self,
        lint: &'static Lint,
        id: ast::NodeId,
        sp: S,
        msg: &str,
    ) {
        self.add_lint(lint, id, sp.into(), msg, BuiltinLintDiagnostics::Normal)
    }

    pub fn buffer_lint_with_diagnostic<S: Into<MultiSpan>>(
        &mut self,
        lint: &'static Lint,
        id: ast::NodeId,
        sp: S,
        msg: &str,
        diagnostic: BuiltinLintDiagnostics,
    ) {
        self.add_lint(lint, id, sp.into(), msg, diagnostic)
    }
}

pub fn struct_lint_level<'a>(sess: &'a Session,
                             lint: &'static Lint,
                             level: Level,
                             src: LintSource,
                             span: Option<MultiSpan>,
                             msg: &str)
    -> DiagnosticBuilder<'a>
{
    let mut err = match (level, span) {
        (Level::Allow, _) => return sess.diagnostic().struct_dummy(),
        (Level::Warn, Some(span)) => sess.struct_span_warn(span, msg),
        (Level::Warn, None) => sess.struct_warn(msg),
        (Level::Deny, Some(span)) |
        (Level::Forbid, Some(span)) => sess.struct_span_err(span, msg),
        (Level::Deny, None) |
        (Level::Forbid, None) => sess.struct_err(msg),
    };

    // Check for future incompatibility lints and issue a stronger warning.
    let lint_id = LintId::of(lint);
    let future_incompatible = lint.future_incompatible;

    // If this code originates in a foreign macro, aka something that this crate
    // did not itself author, then it's likely that there's nothing this crate
    // can do about it. We probably want to skip the lint entirely.
    if err.span.primary_spans().iter().any(|s| in_external_macro(sess, *s)) {
        // Any suggestions made here are likely to be incorrect, so anything we
        // emit shouldn't be automatically fixed by rustfix.
        err.allow_suggestions(false);

        // If this is a future incompatible lint it'll become a hard error, so
        // we have to emit *something*. Also allow lints to whitelist themselves
        // on a case-by-case basis for emission in a foreign macro.
        if future_incompatible.is_none() && !lint.report_in_external_macro {
            err.cancel();
            // Don't continue further, since we don't want to have
            // `diag_span_note_once` called for a diagnostic that isn't emitted.
            return err;
        }
    }

    let name = lint.name_lower();
    match src {
        LintSource::Default => {
            sess.diag_note_once(
                &mut err,
                DiagnosticMessageId::from(lint),
                &format!("`#[{}({})]` on by default", level.as_str(), name));
        }
        LintSource::CommandLine(lint_flag_val) => {
            let flag = match level {
                Level::Warn => "-W",
                Level::Deny => "-D",
                Level::Forbid => "-F",
                Level::Allow => panic!(),
            };
            let hyphen_case_lint_name = name.replace("_", "-");
            if lint_flag_val.as_str() == name {
                sess.diag_note_once(
                    &mut err,
                    DiagnosticMessageId::from(lint),
                    &format!("requested on the command line with `{} {}`",
                             flag, hyphen_case_lint_name));
            } else {
                let hyphen_case_flag_val = lint_flag_val.as_str().replace("_", "-");
                sess.diag_note_once(
                    &mut err,
                    DiagnosticMessageId::from(lint),
                    &format!("`{} {}` implied by `{} {}`",
                             flag, hyphen_case_lint_name, flag,
                             hyphen_case_flag_val));
            }
        }
        LintSource::Node(lint_attr_name, src, reason) => {
            if let Some(rationale) = reason {
                err.note(&rationale.as_str());
            }
            sess.diag_span_note_once(&mut err, DiagnosticMessageId::from(lint),
                                     src, "lint level defined here");
            if lint_attr_name.as_str() != name {
                let level_str = level.as_str();
                sess.diag_note_once(&mut err, DiagnosticMessageId::from(lint),
                                    &format!("`#[{}({})]` implied by `#[{}({})]`",
                                             level_str, name, level_str, lint_attr_name));
            }
        }
    }

    err.code(DiagnosticId::Lint(name));

    if let Some(future_incompatible) = future_incompatible {
        const STANDARD_MESSAGE: &str =
            "this was previously accepted by the compiler but is being phased out; \
             it will become a hard error";

        let explanation = if lint_id == LintId::of(builtin::UNSTABLE_NAME_COLLISIONS) {
            "once this method is added to the standard library, \
             the ambiguity may cause an error or change in behavior!"
                .to_owned()
        } else if lint_id == LintId::of(builtin::MUTABLE_BORROW_RESERVATION_CONFLICT) {
            "this borrowing pattern was not meant to be accepted, \
             and may become a hard error in the future"
                .to_owned()
        } else if let Some(edition) = future_incompatible.edition {
            format!("{} in the {} edition!", STANDARD_MESSAGE, edition)
        } else {
            format!("{} in a future release!", STANDARD_MESSAGE)
        };
        let citation = format!("for more information, see {}",
                               future_incompatible.reference);
        err.warn(&explanation);
        err.note(&citation);
    }

    return err
}

pub fn maybe_lint_level_root(tcx: TyCtxt<'_>, id: hir::HirId) -> bool {
    let attrs = tcx.hir().attrs(id);
    attrs.iter().any(|attr| Level::from_symbol(attr.name_or_empty()).is_some())
}

fn lint_levels(tcx: TyCtxt<'_>, cnum: CrateNum) -> &LintLevelMap {
    assert_eq!(cnum, LOCAL_CRATE);
    let store = &tcx.lint_store;
    let mut builder = LintLevelMapBuilder {
        levels: LintLevelSets::builder(tcx.sess, false, &store),
        tcx: tcx,
        store: store,
    };
    let krate = tcx.hir().krate();

    let push = builder.levels.push(&krate.attrs, &store);
    builder.levels.register_id(hir::CRATE_HIR_ID);
    for macro_def in &krate.exported_macros {
       builder.levels.register_id(macro_def.hir_id);
    }
    intravisit::walk_crate(&mut builder, krate);
    builder.levels.pop(push);

    tcx.arena.alloc(builder.levels.build_map())
}

struct LintLevelMapBuilder<'a, 'tcx> {
    levels: levels::LintLevelsBuilder<'tcx>,
    tcx: TyCtxt<'tcx>,
    store: &'a LintStore,
}

impl LintLevelMapBuilder<'_, '_> {
    fn with_lint_attrs<F>(&mut self,
                          id: hir::HirId,
                          attrs: &[ast::Attribute],
                          f: F)
        where F: FnOnce(&mut Self)
    {
        let push = self.levels.push(attrs, self.store);
        if push.changed {
            self.levels.register_id(id);
        }
        f(self);
        self.levels.pop(push);
    }
}

impl intravisit::Visitor<'tcx> for LintLevelMapBuilder<'_, 'tcx> {
    fn nested_visit_map<'this>(&'this mut self) -> intravisit::NestedVisitorMap<'this, 'tcx> {
        intravisit::NestedVisitorMap::All(&self.tcx.hir())
    }

    fn visit_param(&mut self, param: &'tcx hir::Param) {
        self.with_lint_attrs(param.hir_id, &param.attrs, |builder| {
            intravisit::walk_param(builder, param);
        });
    }

    fn visit_item(&mut self, it: &'tcx hir::Item) {
        self.with_lint_attrs(it.hir_id, &it.attrs, |builder| {
            intravisit::walk_item(builder, it);
        });
    }

    fn visit_foreign_item(&mut self, it: &'tcx hir::ForeignItem) {
        self.with_lint_attrs(it.hir_id, &it.attrs, |builder| {
            intravisit::walk_foreign_item(builder, it);
        })
    }

    fn visit_expr(&mut self, e: &'tcx hir::Expr) {
        self.with_lint_attrs(e.hir_id, &e.attrs, |builder| {
            intravisit::walk_expr(builder, e);
        })
    }

    fn visit_struct_field(&mut self, s: &'tcx hir::StructField) {
        self.with_lint_attrs(s.hir_id, &s.attrs, |builder| {
            intravisit::walk_struct_field(builder, s);
        })
    }

    fn visit_variant(&mut self,
                     v: &'tcx hir::Variant,
                     g: &'tcx hir::Generics,
                     item_id: hir::HirId) {
        self.with_lint_attrs(v.id, &v.attrs, |builder| {
            intravisit::walk_variant(builder, v, g, item_id);
        })
    }

    fn visit_local(&mut self, l: &'tcx hir::Local) {
        self.with_lint_attrs(l.hir_id, &l.attrs, |builder| {
            intravisit::walk_local(builder, l);
        })
    }

    fn visit_arm(&mut self, a: &'tcx hir::Arm) {
        self.with_lint_attrs(a.hir_id, &a.attrs, |builder| {
            intravisit::walk_arm(builder, a);
        })
    }

    fn visit_trait_item(&mut self, trait_item: &'tcx hir::TraitItem) {
        self.with_lint_attrs(trait_item.hir_id, &trait_item.attrs, |builder| {
            intravisit::walk_trait_item(builder, trait_item);
        });
    }

    fn visit_impl_item(&mut self, impl_item: &'tcx hir::ImplItem) {
        self.with_lint_attrs(impl_item.hir_id, &impl_item.attrs, |builder| {
            intravisit::walk_impl_item(builder, impl_item);
        });
    }
}

pub fn provide(providers: &mut Providers<'_>) {
    providers.lint_levels = lint_levels;
}

/// Returns whether `span` originates in a foreign crate's external macro.
///
/// This is used to test whether a lint should not even begin to figure out whether it should
/// be reported on the current node.
pub fn in_external_macro(sess: &Session, span: Span) -> bool {
    let expn_data = span.ctxt().outer_expn_data();
    match expn_data.kind {
        ExpnKind::Root | ExpnKind::Desugaring(DesugaringKind::ForLoop) => false,
        ExpnKind::AstPass(_) | ExpnKind::Desugaring(_) => true, // well, it's "external"
        ExpnKind::Macro(MacroKind::Bang, _) => {
            if expn_data.def_site.is_dummy() {
                // Dummy span for the `def_site` means it's an external macro.
                return true;
            }
            match sess.source_map().span_to_snippet(expn_data.def_site) {
                Ok(code) => !code.starts_with("macro_rules"),
                // No snippet means external macro or compiler-builtin expansion.
                Err(_) => true,
            }
        }
        ExpnKind::Macro(..) => true, // definitely a plugin
    }
}

/// Returns `true` if `span` originates in a derive-macro's expansion.
pub fn in_derive_expansion(span: Span) -> bool {
    if let ExpnKind::Macro(MacroKind::Derive, _) = span.ctxt().outer_expn_data().kind {
        return true;
    }
    false
}
