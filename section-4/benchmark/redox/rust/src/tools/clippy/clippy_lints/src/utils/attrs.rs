use rustc::session::Session;
use rustc_errors::Applicability;
use std::str::FromStr;
use syntax::ast;

/// Deprecation status of attributes known by Clippy.
#[allow(dead_code)]
pub enum DeprecationStatus {
    /// Attribute is deprecated
    Deprecated,
    /// Attribute is deprecated and was replaced by the named attribute
    Replaced(&'static str),
    None,
}

pub const BUILTIN_ATTRIBUTES: &[(&str, DeprecationStatus)] = &[
    ("author", DeprecationStatus::None),
    ("cognitive_complexity", DeprecationStatus::None),
    (
        "cyclomatic_complexity",
        DeprecationStatus::Replaced("cognitive_complexity"),
    ),
    ("dump", DeprecationStatus::None),
];

pub struct LimitStack {
    stack: Vec<u64>,
}

impl Drop for LimitStack {
    fn drop(&mut self) {
        assert_eq!(self.stack.len(), 1);
    }
}

impl LimitStack {
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self { stack: vec![limit] }
    }
    pub fn limit(&self) -> u64 {
        *self.stack.last().expect("there should always be a value in the stack")
    }
    pub fn push_attrs(&mut self, sess: &Session, attrs: &[ast::Attribute], name: &'static str) {
        let stack = &mut self.stack;
        parse_attrs(sess, attrs, name, |val| stack.push(val));
    }
    pub fn pop_attrs(&mut self, sess: &Session, attrs: &[ast::Attribute], name: &'static str) {
        let stack = &mut self.stack;
        parse_attrs(sess, attrs, name, |val| assert_eq!(stack.pop(), Some(val)));
    }
}

pub fn get_attr<'a>(
    sess: &'a Session,
    attrs: &'a [ast::Attribute],
    name: &'static str,
) -> impl Iterator<Item = &'a ast::Attribute> {
    attrs.iter().filter(move |attr| {
        let attr = if let ast::AttrKind::Normal(ref attr) = attr.kind {
            attr
        } else {
            return false;
        };
        let attr_segments = &attr.path.segments;
        if attr_segments.len() == 2 && attr_segments[0].ident.to_string() == "clippy" {
            if let Some(deprecation_status) =
                BUILTIN_ATTRIBUTES
                    .iter()
                    .find_map(|(builtin_name, deprecation_status)| {
                        if *builtin_name == attr_segments[1].ident.to_string() {
                            Some(deprecation_status)
                        } else {
                            None
                        }
                    })
            {
                let mut db = sess.struct_span_err(attr_segments[1].ident.span, "Usage of deprecated attribute");
                match *deprecation_status {
                    DeprecationStatus::Deprecated => {
                        db.emit();
                        false
                    },
                    DeprecationStatus::Replaced(new_name) => {
                        db.span_suggestion(
                            attr_segments[1].ident.span,
                            "consider using",
                            new_name.to_string(),
                            Applicability::MachineApplicable,
                        );
                        db.emit();
                        false
                    },
                    DeprecationStatus::None => {
                        db.cancel();
                        attr_segments[1].ident.to_string() == name
                    },
                }
            } else {
                sess.span_err(attr_segments[1].ident.span, "Usage of unknown attribute");
                false
            }
        } else {
            false
        }
    })
}

fn parse_attrs<F: FnMut(u64)>(sess: &Session, attrs: &[ast::Attribute], name: &'static str, mut f: F) {
    for attr in get_attr(sess, attrs, name) {
        if let Some(ref value) = attr.value_str() {
            if let Ok(value) = FromStr::from_str(&value.as_str()) {
                f(value)
            } else {
                sess.span_err(attr.span, "not a number");
            }
        } else {
            sess.span_err(attr.span, "bad clippy attribute");
        }
    }
}

/// Return true if the attributes contain any of `proc_macro`,
/// `proc_macro_derive` or `proc_macro_attribute`, false otherwise
pub fn is_proc_macro(attrs: &[ast::Attribute]) -> bool {
    use syntax_pos::Symbol;

    let syms = [
        Symbol::intern("proc_macro"),
        Symbol::intern("proc_macro_derive"),
        Symbol::intern("proc_macro_attribute"),
    ];
    attrs.iter().any(|attr| syms.iter().any(move |&s| attr.check_name(s)))
}
