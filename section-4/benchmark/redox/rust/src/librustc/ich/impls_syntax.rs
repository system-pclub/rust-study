//! This module contains `HashStable` implementations for various data types
//! from libsyntax in no particular order.

use crate::ich::StableHashingContext;

use std::hash as std_hash;
use std::mem;

use syntax::ast;
use syntax::feature_gate;
use syntax::token;
use syntax::tokenstream;
use syntax_pos::SourceFile;

use crate::hir::def_id::{DefId, CrateNum, CRATE_DEF_INDEX};

use smallvec::SmallVec;
use rustc_data_structures::stable_hasher::{HashStable, StableHasher};

impl_stable_hash_for!(struct ::syntax::ast::Lit {
    kind,
    token,
    span
});

impl_stable_hash_for_spanned!(::syntax::ast::LitKind);

impl_stable_hash_for!(struct ::syntax::ast::Lifetime { id, ident });

impl<'a> HashStable<StableHashingContext<'a>> for [ast::Attribute] {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        if self.len() == 0 {
            self.len().hash_stable(hcx, hasher);
            return
        }

        // Some attributes are always ignored during hashing.
        let filtered: SmallVec<[&ast::Attribute; 8]> = self
            .iter()
            .filter(|attr| {
                !attr.is_doc_comment() &&
                !attr.ident().map_or(false, |ident| hcx.is_ignored_attr(ident.name))
            })
            .collect();

        filtered.len().hash_stable(hcx, hasher);
        for attr in filtered {
            attr.hash_stable(hcx, hasher);
        }
    }
}

impl<'a> HashStable<StableHashingContext<'a>> for ast::Path {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        self.segments.len().hash_stable(hcx, hasher);
        for segment in &self.segments {
            segment.ident.name.hash_stable(hcx, hasher);
        }
    }
}

impl_stable_hash_for!(struct ::syntax::ast::AttrItem {
    path,
    tokens,
});

impl<'a> HashStable<StableHashingContext<'a>> for ast::Attribute {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        // Make sure that these have been filtered out.
        debug_assert!(!self.ident().map_or(false, |ident| hcx.is_ignored_attr(ident.name)));
        debug_assert!(!self.is_doc_comment());

        let ast::Attribute { kind, id: _, style, span } = self;
        if let ast::AttrKind::Normal(item) = kind {
            item.hash_stable(hcx, hasher);
            style.hash_stable(hcx, hasher);
            span.hash_stable(hcx, hasher);
        } else {
            unreachable!();
        }
    }
}

impl<'a> HashStable<StableHashingContext<'a>>
for tokenstream::TokenTree {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        mem::discriminant(self).hash_stable(hcx, hasher);
        match *self {
            tokenstream::TokenTree::Token(ref token) => {
                token.hash_stable(hcx, hasher);
            }
            tokenstream::TokenTree::Delimited(span, delim, ref tts) => {
                span.hash_stable(hcx, hasher);
                std_hash::Hash::hash(&delim, hasher);
                for sub_tt in tts.trees() {
                    sub_tt.hash_stable(hcx, hasher);
                }
            }
        }
    }
}

impl<'a> HashStable<StableHashingContext<'a>>
for tokenstream::TokenStream {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        for sub_tt in self.trees() {
            sub_tt.hash_stable(hcx, hasher);
        }
    }
}

impl<'a> HashStable<StableHashingContext<'a>> for token::TokenKind {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        mem::discriminant(self).hash_stable(hcx, hasher);
        match *self {
            token::Eq |
            token::Lt |
            token::Le |
            token::EqEq |
            token::Ne |
            token::Ge |
            token::Gt |
            token::AndAnd |
            token::OrOr |
            token::Not |
            token::Tilde |
            token::At |
            token::Dot |
            token::DotDot |
            token::DotDotDot |
            token::DotDotEq |
            token::Comma |
            token::Semi |
            token::Colon |
            token::ModSep |
            token::RArrow |
            token::LArrow |
            token::FatArrow |
            token::Pound |
            token::Dollar |
            token::Question |
            token::SingleQuote |
            token::Whitespace |
            token::Comment |
            token::Eof => {}

            token::BinOp(bin_op_token) |
            token::BinOpEq(bin_op_token) => {
                std_hash::Hash::hash(&bin_op_token, hasher);
            }

            token::OpenDelim(delim_token) |
            token::CloseDelim(delim_token) => {
                std_hash::Hash::hash(&delim_token, hasher);
            }
            token::Literal(lit) => lit.hash_stable(hcx, hasher),

            token::Ident(name, is_raw) => {
                name.hash_stable(hcx, hasher);
                is_raw.hash_stable(hcx, hasher);
            }
            token::Lifetime(name) => name.hash_stable(hcx, hasher),

            token::Interpolated(_) => {
                bug!("interpolated tokens should not be present in the HIR")
            }

            token::DocComment(val) |
            token::Shebang(val) |
            token::Unknown(val) => val.hash_stable(hcx, hasher),
        }
    }
}

impl_stable_hash_for!(struct token::Token {
    kind,
    span
});

impl_stable_hash_for!(enum ::syntax::ast::NestedMetaItem {
    MetaItem(meta_item),
    Literal(lit)
});

impl_stable_hash_for!(struct ::syntax::ast::MetaItem {
    path,
    kind,
    span
});

impl_stable_hash_for!(enum ::syntax::ast::MetaItemKind {
    Word,
    List(nested_items),
    NameValue(lit)
});

impl_stable_hash_for!(struct ::syntax_pos::hygiene::ExpnData {
    kind,
    parent -> _,
    call_site,
    def_site,
    allow_internal_unstable,
    allow_internal_unsafe,
    local_inner_macros,
    edition
});

impl<'a> HashStable<StableHashingContext<'a>> for SourceFile {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        let SourceFile {
            name: _, // We hash the smaller name_hash instead of this
            name_hash,
            name_was_remapped,
            unmapped_path: _,
            crate_of_origin,
            // Do not hash the source as it is not encoded
            src: _,
            src_hash,
            external_src: _,
            start_pos,
            end_pos: _,
            ref lines,
            ref multibyte_chars,
            ref non_narrow_chars,
            ref normalized_pos,
        } = *self;

        (name_hash as u64).hash_stable(hcx, hasher);
        name_was_remapped.hash_stable(hcx, hasher);

        DefId {
            krate: CrateNum::from_u32(crate_of_origin),
            index: CRATE_DEF_INDEX,
        }.hash_stable(hcx, hasher);

        src_hash.hash_stable(hcx, hasher);

        // We only hash the relative position within this source_file
        lines.len().hash_stable(hcx, hasher);
        for &line in lines.iter() {
            stable_byte_pos(line, start_pos).hash_stable(hcx, hasher);
        }

        // We only hash the relative position within this source_file
        multibyte_chars.len().hash_stable(hcx, hasher);
        for &char_pos in multibyte_chars.iter() {
            stable_multibyte_char(char_pos, start_pos).hash_stable(hcx, hasher);
        }

        non_narrow_chars.len().hash_stable(hcx, hasher);
        for &char_pos in non_narrow_chars.iter() {
            stable_non_narrow_char(char_pos, start_pos).hash_stable(hcx, hasher);
        }

        normalized_pos.len().hash_stable(hcx, hasher);
        for &char_pos in normalized_pos.iter() {
            stable_normalized_pos(char_pos, start_pos).hash_stable(hcx, hasher);
        }

    }
}

fn stable_byte_pos(pos: ::syntax_pos::BytePos,
                   source_file_start: ::syntax_pos::BytePos)
                   -> u32 {
    pos.0 - source_file_start.0
}

fn stable_multibyte_char(mbc: ::syntax_pos::MultiByteChar,
                         source_file_start: ::syntax_pos::BytePos)
                         -> (u32, u32) {
    let ::syntax_pos::MultiByteChar {
        pos,
        bytes,
    } = mbc;

    (pos.0 - source_file_start.0, bytes as u32)
}

fn stable_non_narrow_char(swc: ::syntax_pos::NonNarrowChar,
                          source_file_start: ::syntax_pos::BytePos)
                          -> (u32, u32) {
    let pos = swc.pos();
    let width = swc.width();

    (pos.0 - source_file_start.0, width as u32)
}

fn stable_normalized_pos(np: ::syntax_pos::NormalizedPos,
                         source_file_start: ::syntax_pos::BytePos)
                         -> (u32, u32) {
    let ::syntax_pos::NormalizedPos {
        pos,
        diff
    } = np;

    (pos.0 - source_file_start.0, diff)
}


impl<'tcx> HashStable<StableHashingContext<'tcx>> for feature_gate::Features {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'tcx>, hasher: &mut StableHasher) {
        // Unfortunately we cannot exhaustively list fields here, since the
        // struct is macro generated.
        self.declared_lang_features.hash_stable(hcx, hasher);
        self.declared_lib_features.hash_stable(hcx, hasher);

        self.walk_feature_fields(|feature_name, value| {
            feature_name.hash_stable(hcx, hasher);
            value.hash_stable(hcx, hasher);
        });
    }
}
