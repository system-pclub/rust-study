use super::BackendTypes;
use crate::mir::place::PlaceRef;
use rustc::hir::{GlobalAsm, InlineAsmInner};
use syntax_pos::Span;

pub trait AsmBuilderMethods<'tcx>: BackendTypes {
    /// Take an inline assembly expression and splat it out via LLVM
    fn codegen_inline_asm(
        &mut self,
        ia: &InlineAsmInner,
        outputs: Vec<PlaceRef<'tcx, Self::Value>>,
        inputs: Vec<Self::Value>,
        span: Span,
    ) -> bool;
}

pub trait AsmMethods {
    fn codegen_global_asm(&self, ga: &GlobalAsm);
}
