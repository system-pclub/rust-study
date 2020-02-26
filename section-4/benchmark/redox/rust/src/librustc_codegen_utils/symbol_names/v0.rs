use rustc::hir;
use rustc::hir::def_id::{CrateNum, DefId};
use rustc::hir::map::{DefPathData, DisambiguatedDefPathData};
use rustc::ty::{self, Ty, TyCtxt, TypeFoldable, Instance};
use rustc::ty::print::{Printer, Print};
use rustc::ty::subst::{GenericArg, Subst, GenericArgKind};
use rustc_data_structures::base_n;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_target::spec::abi::Abi;
use syntax::ast::{IntTy, UintTy, FloatTy};

use std::fmt::Write;
use std::ops::Range;

pub(super) fn mangle(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    instantiating_crate: Option<CrateNum>,
) -> String {
    let def_id = instance.def_id();
    // FIXME(eddyb) this should ideally not be needed.
    let substs =
        tcx.normalize_erasing_regions(ty::ParamEnv::reveal_all(), instance.substs);

    let prefix = "_R";
    let mut cx = SymbolMangler {
        tcx,
        compress: Some(Box::new(CompressionCaches {
            start_offset: prefix.len(),

            paths: FxHashMap::default(),
            types: FxHashMap::default(),
            consts: FxHashMap::default(),
        })),
        binders: vec![],
        out: String::from(prefix),
    };
    cx = if instance.is_vtable_shim() {
        cx.path_append_ns(
            |cx| cx.print_def_path(def_id, substs),
            'S',
            0,
            "",
        ).unwrap()
    } else {
        cx.print_def_path(def_id, substs).unwrap()
    };
    if let Some(instantiating_crate) = instantiating_crate {
        cx = cx.print_def_path(instantiating_crate.as_def_id(), &[]).unwrap();
    }
    cx.out
}

struct CompressionCaches<'tcx> {
    // The length of the prefix in `out` (e.g. 2 for `_R`).
    start_offset: usize,

    // The values are start positions in `out`, in bytes.
    paths: FxHashMap<(DefId, &'tcx [GenericArg<'tcx>]), usize>,
    types: FxHashMap<Ty<'tcx>, usize>,
    consts: FxHashMap<&'tcx ty::Const<'tcx>, usize>,
}

struct BinderLevel {
    /// The range of distances from the root of what's
    /// being printed, to the lifetimes in a binder.
    /// Specifically, a `BrAnon(i)` lifetime has depth
    /// `lifetime_depths.start + i`, going away from the
    /// the root and towards its use site, as `i` increases.
    /// This is used to flatten rustc's pairing of `BrAnon`
    /// (intra-binder disambiguation) with a `DebruijnIndex`
    /// (binder addressing), to "true" de Bruijn indices,
    /// by subtracting the depth of a certain lifetime, from
    /// the innermost depth at its use site.
    lifetime_depths: Range<u32>,
}

struct SymbolMangler<'tcx> {
    tcx: TyCtxt<'tcx>,
    compress: Option<Box<CompressionCaches<'tcx>>>,
    binders: Vec<BinderLevel>,
    out: String,
}

impl SymbolMangler<'tcx> {
    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Push a `_`-terminated base 62 integer, using the format
    /// specified in the RFC as `<base-62-number>`, that is:
    /// * `x = 0` is encoded as just the `"_"` terminator
    /// * `x > 0` is encoded as `x - 1` in base 62, followed by `"_"`,
    ///   e.g. `1` becomes `"0_"`, `62` becomes `"Z_"`, etc.
    fn push_integer_62(&mut self, x: u64) {
        if let Some(x) = x.checked_sub(1) {
            base_n::push_str(x as u128, 62, &mut self.out);
        }
        self.push("_");
    }

    /// Push a `tag`-prefixed base 62 integer, when larger than `0`, that is:
    /// * `x = 0` is encoded as `""` (nothing)
    /// * `x > 0` is encoded as the `tag` followed by `push_integer_62(x - 1)`
    ///   e.g. `1` becomes `tag + "_"`, `2` becomes `tag + "0_"`, etc.
    fn push_opt_integer_62(&mut self, tag: &str, x: u64) {
        if let Some(x) = x.checked_sub(1) {
            self.push(tag);
            self.push_integer_62(x);
        }
    }

    fn push_disambiguator(&mut self, dis: u64) {
        self.push_opt_integer_62("s", dis);
    }

    fn push_ident(&mut self, ident: &str) {
        let mut use_punycode = false;
        for b in ident.bytes() {
            match b {
                b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => {}
                0x80..=0xff => use_punycode = true,
                _ => bug!("symbol_names: bad byte {} in ident {:?}", b, ident),
            }
        }

        let punycode_string;
        let ident = if use_punycode {
            self.push("u");

            // FIXME(eddyb) we should probably roll our own punycode implementation.
            let mut punycode_bytes = match ::punycode::encode(ident) {
                Ok(s) => s.into_bytes(),
                Err(()) => bug!("symbol_names: punycode encoding failed for ident {:?}", ident),
            };

            // Replace `-` with `_`.
            if let Some(c) = punycode_bytes.iter_mut().rfind(|&&mut c| c == b'-') {
                *c = b'_';
            }

            // FIXME(eddyb) avoid rechecking UTF-8 validity.
            punycode_string = String::from_utf8(punycode_bytes).unwrap();
            &punycode_string
        } else {
            ident
        };

        let _ = write!(self.out, "{}", ident.len());

        // Write a separating `_` if necessary (leading digit or `_`).
        match ident.chars().next() {
            Some('_') | Some('0'..='9') => {
                self.push("_");
            }
            _ => {}
        }

        self.push(ident);
    }

    fn path_append_ns(
        mut self,
        print_prefix: impl FnOnce(Self) -> Result<Self, !>,
        ns: char,
        disambiguator: u64,
        name: &str,
    ) -> Result<Self, !> {
        self.push("N");
        self.out.push(ns);
        self = print_prefix(self)?;
        self.push_disambiguator(disambiguator as u64);
        self.push_ident(name);
        Ok(self)
    }

    fn print_backref(mut self, i: usize) -> Result<Self, !> {
        self.push("B");
        self.push_integer_62((i - self.compress.as_ref().unwrap().start_offset) as u64);
        Ok(self)
    }

    fn in_binder<T>(
        mut self,
        value: &ty::Binder<T>,
        print_value: impl FnOnce(Self, &T) -> Result<Self, !>
    ) -> Result<Self, !>
        where T: TypeFoldable<'tcx>
    {
        let regions = if value.has_late_bound_regions() {
            self.tcx.collect_referenced_late_bound_regions(value)
        } else {
            FxHashSet::default()
        };

        let mut lifetime_depths =
            self.binders.last().map(|b| b.lifetime_depths.end).map_or(0..0, |i| i..i);

        let lifetimes = regions.into_iter().map(|br| {
            match br {
                ty::BrAnon(i) => {
                    // FIXME(eddyb) for some reason, `anonymize_late_bound_regions` starts at `1`.
                    assert_ne!(i, 0);
                    i - 1
                },
                _ => bug!("symbol_names: non-anonymized region `{:?}` in `{:?}`", br, value),
            }
        }).max().map_or(0, |max| max + 1);

        self.push_opt_integer_62("G", lifetimes as u64);
        lifetime_depths.end += lifetimes;

        self.binders.push(BinderLevel { lifetime_depths });
        self = print_value(self, value.skip_binder())?;
        self.binders.pop();

        Ok(self)
    }
}

impl Printer<'tcx> for SymbolMangler<'tcx> {
    type Error = !;

    type Path = Self;
    type Region = Self;
    type Type = Self;
    type DynExistential = Self;
    type Const = Self;

    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn print_def_path(
        mut self,
        def_id: DefId,
        substs: &'tcx [GenericArg<'tcx>],
    ) -> Result<Self::Path, Self::Error> {
        if let Some(&i) = self.compress.as_ref().and_then(|c| c.paths.get(&(def_id, substs))) {
            return self.print_backref(i);
        }
        let start = self.out.len();

        self = self.default_print_def_path(def_id, substs)?;

        // Only cache paths that do not refer to an enclosing
        // binder (which would change depending on context).
        if !substs.iter().any(|k| k.has_escaping_bound_vars()) {
            if let Some(c) = &mut self.compress {
                c.paths.insert((def_id, substs), start);
            }
        }
        Ok(self)
    }

    fn print_impl_path(
        self,
        impl_def_id: DefId,
        substs: &'tcx [GenericArg<'tcx>],
        mut self_ty: Ty<'tcx>,
        mut impl_trait_ref: Option<ty::TraitRef<'tcx>>,
    ) -> Result<Self::Path, Self::Error> {
        let key = self.tcx.def_key(impl_def_id);
        let parent_def_id = DefId { index: key.parent.unwrap(), ..impl_def_id };

        let mut param_env = self.tcx.param_env(impl_def_id)
            .with_reveal_all();
        if !substs.is_empty() {
            param_env = param_env.subst(self.tcx, substs);
        }

        match &mut impl_trait_ref {
            Some(impl_trait_ref) => {
                assert_eq!(impl_trait_ref.self_ty(), self_ty);
                *impl_trait_ref =
                    self.tcx.normalize_erasing_regions(param_env, *impl_trait_ref);
                self_ty = impl_trait_ref.self_ty();
            }
            None => {
                self_ty = self.tcx.normalize_erasing_regions(param_env, self_ty);
            }
        }

        self.path_append_impl(
            |cx| cx.print_def_path(parent_def_id, &[]),
            &key.disambiguated_data,
            self_ty,
            impl_trait_ref,
        )
    }

    fn print_region(
        mut self,
        region: ty::Region<'_>,
    ) -> Result<Self::Region, Self::Error> {
        let i = match *region {
            // Erased lifetimes use the index 0, for a
            // shorter mangling of `L_`.
            ty::ReErased => 0,

            // Late-bound lifetimes use indices starting at 1,
            // see `BinderLevel` for more details.
            ty::ReLateBound(debruijn, ty::BrAnon(i)) => {
                // FIXME(eddyb) for some reason, `anonymize_late_bound_regions` starts at `1`.
                assert_ne!(i, 0);
                let i = i - 1;

                let binder = &self.binders[self.binders.len() - 1 - debruijn.index()];
                let depth = binder.lifetime_depths.start + i;

                1 + (self.binders.last().unwrap().lifetime_depths.end - 1 - depth)
            }

            _ => bug!("symbol_names: non-erased region `{:?}`", region),
        };
        self.push("L");
        self.push_integer_62(i as u64);
        Ok(self)
    }

    fn print_type(
        mut self,
        ty: Ty<'tcx>,
    ) -> Result<Self::Type, Self::Error> {
        // Basic types, never cached (single-character).
        let basic_type = match ty.kind {
            ty::Bool => "b",
            ty::Char => "c",
            ty::Str => "e",
            ty::Tuple(_) if ty.is_unit() => "u",
            ty::Int(IntTy::I8) => "a",
            ty::Int(IntTy::I16) => "s",
            ty::Int(IntTy::I32) => "l",
            ty::Int(IntTy::I64) => "x",
            ty::Int(IntTy::I128) => "n",
            ty::Int(IntTy::Isize) => "i",
            ty::Uint(UintTy::U8) => "h",
            ty::Uint(UintTy::U16) => "t",
            ty::Uint(UintTy::U32) => "m",
            ty::Uint(UintTy::U64) => "y",
            ty::Uint(UintTy::U128) => "o",
            ty::Uint(UintTy::Usize) => "j",
            ty::Float(FloatTy::F32) => "f",
            ty::Float(FloatTy::F64) => "d",
            ty::Never => "z",

            // Placeholders (should be demangled as `_`).
            ty::Param(_) | ty::Bound(..) | ty::Placeholder(_) |
            ty::Infer(_) | ty::Error => "p",

            _ => "",
        };
        if !basic_type.is_empty() {
            self.push(basic_type);
            return Ok(self);
        }

        if let Some(&i) = self.compress.as_ref().and_then(|c| c.types.get(&ty)) {
            return self.print_backref(i);
        }
        let start = self.out.len();

        match ty.kind {
            // Basic types, handled above.
            ty::Bool | ty::Char | ty::Str |
            ty::Int(_) | ty::Uint(_) | ty::Float(_) |
            ty::Never => unreachable!(),
            ty::Tuple(_) if ty.is_unit() => unreachable!(),

            // Placeholders, also handled as part of basic types.
            ty::Param(_) | ty::Bound(..) | ty::Placeholder(_) |
            ty::Infer(_) | ty::Error => unreachable!(),

            ty::Ref(r, ty, mutbl) => {
                self.push(match mutbl {
                    hir::Mutability::Immutable => "R",
                    hir::Mutability::Mutable => "Q",
                });
                if *r != ty::ReErased {
                    self = r.print(self)?;
                }
                self = ty.print(self)?;
            }

            ty::RawPtr(mt) => {
                self.push(match mt.mutbl {
                    hir::Mutability::Immutable => "P",
                    hir::Mutability::Mutable => "O",
                });
                self = mt.ty.print(self)?;
            }

            ty::Array(ty, len) => {
                self.push("A");
                self = ty.print(self)?;
                self = self.print_const(len)?;
            }
            ty::Slice(ty) => {
                self.push("S");
                self = ty.print(self)?;
            }

            ty::Tuple(tys) => {
                self.push("T");
                for ty in tys.iter().map(|k| k.expect_ty()) {
                    self = ty.print(self)?;
                }
                self.push("E");
            }

            // Mangle all nominal types as paths.
            ty::Adt(&ty::AdtDef { did: def_id, .. }, substs) |
            ty::FnDef(def_id, substs) |
            ty::Opaque(def_id, substs) |
            ty::Projection(ty::ProjectionTy { item_def_id: def_id, substs }) |
            ty::UnnormalizedProjection(ty::ProjectionTy { item_def_id: def_id, substs }) |
            ty::Closure(def_id, substs) |
            ty::Generator(def_id, substs, _) => {
                self = self.print_def_path(def_id, substs)?;
            }
            ty::Foreign(def_id) => {
                self = self.print_def_path(def_id, &[])?;
            }

            ty::FnPtr(sig) => {
                self.push("F");
                self = self.in_binder(&sig, |mut cx, sig| {
                    if sig.unsafety == hir::Unsafety::Unsafe {
                        cx.push("U");
                    }
                    match sig.abi {
                        Abi::Rust => {}
                        Abi::C => cx.push("KC"),
                        abi => {
                            cx.push("K");
                            let name = abi.name();
                            if name.contains('-') {
                                cx.push_ident(&name.replace('-', "_"));
                            } else {
                                cx.push_ident(name);
                            }
                        }
                    }
                    for &ty in sig.inputs() {
                        cx = ty.print(cx)?;
                    }
                    if sig.c_variadic {
                        cx.push("v");
                    }
                    cx.push("E");
                    sig.output().print(cx)
                })?;
            }

            ty::Dynamic(predicates, r) => {
                self.push("D");
                self = self.in_binder(&predicates, |cx, predicates| {
                    cx.print_dyn_existential(predicates)
                })?;
                self = r.print(self)?;
            }

            ty::GeneratorWitness(_) => {
                bug!("symbol_names: unexpected `GeneratorWitness`")
            }
        }

        // Only cache types that do not refer to an enclosing
        // binder (which would change depending on context).
        if !ty.has_escaping_bound_vars() {
            if let Some(c) = &mut self.compress {
                c.types.insert(ty, start);
            }
        }
        Ok(self)
    }

    fn print_dyn_existential(
        mut self,
        predicates: &'tcx ty::List<ty::ExistentialPredicate<'tcx>>,
    ) -> Result<Self::DynExistential, Self::Error> {
        for predicate in predicates {
            match *predicate {
                ty::ExistentialPredicate::Trait(trait_ref) => {
                    // Use a type that can't appear in defaults of type parameters.
                    let dummy_self = self.tcx.mk_ty_infer(ty::FreshTy(0));
                    let trait_ref = trait_ref.with_self_ty(self.tcx, dummy_self);
                    self = self.print_def_path(trait_ref.def_id, trait_ref.substs)?;
                }
                ty::ExistentialPredicate::Projection(projection) => {
                    let name = self.tcx.associated_item(projection.item_def_id).ident;
                    self.push("p");
                    self.push_ident(&name.as_str());
                    self = projection.ty.print(self)?;
                }
                ty::ExistentialPredicate::AutoTrait(def_id) => {
                    self = self.print_def_path(def_id, &[])?;
                }
            }
        }
        self.push("E");
        Ok(self)
    }

    fn print_const(
        mut self,
        ct: &'tcx ty::Const<'tcx>,
    ) -> Result<Self::Const, Self::Error> {
        if let Some(&i) = self.compress.as_ref().and_then(|c| c.consts.get(&ct)) {
            return self.print_backref(i);
        }
        let start = self.out.len();

        match ct.ty.kind {
            ty::Uint(_) => {}
            _ => {
                bug!("symbol_names: unsupported constant of type `{}` ({:?})",
                    ct.ty, ct);
            }
        }
        self = ct.ty.print(self)?;

        if let Some(bits) = ct.try_eval_bits(self.tcx, ty::ParamEnv::reveal_all(), ct.ty) {
            let _ = write!(self.out, "{:x}_", bits);
        } else {
            // NOTE(eddyb) despite having the path, we need to
            // encode a placeholder, as the path could refer
            // back to e.g. an `impl` using the constant.
            self.push("p");
        }

        // Only cache consts that do not refer to an enclosing
        // binder (which would change depending on context).
        if !ct.has_escaping_bound_vars() {
            if let Some(c) = &mut self.compress {
                c.consts.insert(ct, start);
            }
        }
        Ok(self)
    }

    fn path_crate(
        mut self,
        cnum: CrateNum,
    ) -> Result<Self::Path, Self::Error> {
        self.push("C");
        let fingerprint = self.tcx.crate_disambiguator(cnum).to_fingerprint();
        self.push_disambiguator(fingerprint.to_smaller_hash());
        let name = self.tcx.original_crate_name(cnum).as_str();
        self.push_ident(&name);
        Ok(self)
    }
    fn path_qualified(
        mut self,
        self_ty: Ty<'tcx>,
        trait_ref: Option<ty::TraitRef<'tcx>>,
    ) -> Result<Self::Path, Self::Error> {
        assert!(trait_ref.is_some());
        let trait_ref = trait_ref.unwrap();

        self.push("Y");
        self = self_ty.print(self)?;
        self.print_def_path(trait_ref.def_id, trait_ref.substs)
    }

    fn path_append_impl(
        mut self,
        print_prefix: impl FnOnce(Self) -> Result<Self::Path, Self::Error>,
        disambiguated_data: &DisambiguatedDefPathData,
        self_ty: Ty<'tcx>,
        trait_ref: Option<ty::TraitRef<'tcx>>,
    ) -> Result<Self::Path, Self::Error> {
        self.push(match trait_ref {
            Some(_) => "X",
            None => "M",
        });
        self.push_disambiguator(disambiguated_data.disambiguator as u64);
        self = print_prefix(self)?;
        self = self_ty.print(self)?;
        if let Some(trait_ref) = trait_ref {
            self = self.print_def_path(trait_ref.def_id, trait_ref.substs)?;
        }
        Ok(self)
    }
    fn path_append(
        self,
        print_prefix: impl FnOnce(Self) -> Result<Self::Path, Self::Error>,
        disambiguated_data: &DisambiguatedDefPathData,
    ) -> Result<Self::Path, Self::Error> {
        let ns = match disambiguated_data.data {
            // Uppercase categories are more stable than lowercase ones.
            DefPathData::TypeNs(_) => 't',
            DefPathData::ValueNs(_) => 'v',
            DefPathData::ClosureExpr => 'C',
            DefPathData::Ctor => 'c',
            DefPathData::AnonConst => 'k',
            DefPathData::ImplTrait => 'i',

            // These should never show up as `path_append` arguments.
            DefPathData::CrateRoot
            | DefPathData::Misc
            | DefPathData::Impl
            | DefPathData::MacroNs(_)
            | DefPathData::LifetimeNs(_) => {
                bug!("symbol_names: unexpected DefPathData: {:?}", disambiguated_data.data)
            }
        };

        let name = disambiguated_data.data.get_opt_name().map(|s| s.as_str());

        self.path_append_ns(
            print_prefix,
            ns,
            disambiguated_data.disambiguator as u64,
            name.as_ref().map_or("", |s| &s[..])
        )
    }
    fn path_generic_args(
        mut self,
        print_prefix: impl FnOnce(Self) -> Result<Self::Path, Self::Error>,
        args: &[GenericArg<'tcx>],
    ) -> Result<Self::Path, Self::Error> {
        // Don't print any regions if they're all erased.
        let print_regions = args.iter().any(|arg| {
            match arg.unpack() {
                GenericArgKind::Lifetime(r) => *r != ty::ReErased,
                _ => false,
            }
        });
        let args = args.iter().cloned().filter(|arg| {
            match arg.unpack() {
                GenericArgKind::Lifetime(_) => print_regions,
                _ => true,
            }
        });

        if args.clone().next().is_none() {
            return print_prefix(self);
        }

        self.push("I");
        self = print_prefix(self)?;
        for arg in args {
            match arg.unpack() {
                GenericArgKind::Lifetime(lt) => {
                    self = lt.print(self)?;
                }
                GenericArgKind::Type(ty) => {
                    self = ty.print(self)?;
                }
                GenericArgKind::Const(c) => {
                    self.push("K");
                    // FIXME(const_generics) implement `ty::print::Print` on `ty::Const`.
                    // self = c.print(self)?;
                    self = self.print_const(c)?;
                }
            }
        }
        self.push("E");

        Ok(self)
    }
}
