//! The representation of a `#[doc(cfg(...))]` attribute.

// FIXME: Once the portability lint RFC is implemented (see tracking issue #41619),
// switch to use those structures instead.

use std::mem;
use std::fmt::{self, Write};
use std::ops;

use syntax::symbol::{Symbol, sym};
use syntax::ast::{MetaItem, MetaItemKind, NestedMetaItem, LitKind};
use syntax::sess::ParseSess;
use syntax::feature_gate::Features;

use syntax_pos::Span;

use crate::html::escape::Escape;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cfg {
    /// Accepts all configurations.
    True,
    /// Denies all configurations.
    False,
    /// A generic configuration option, e.g., `test` or `target_os = "linux"`.
    Cfg(Symbol, Option<Symbol>),
    /// Negates a configuration requirement, i.e., `not(x)`.
    Not(Box<Cfg>),
    /// Union of a list of configuration requirements, i.e., `any(...)`.
    Any(Vec<Cfg>),
    /// Intersection of a list of configuration requirements, i.e., `all(...)`.
    All(Vec<Cfg>),
}

#[derive(PartialEq, Debug)]
pub struct InvalidCfgError {
    pub msg: &'static str,
    pub span: Span,
}

impl Cfg {
    /// Parses a `NestedMetaItem` into a `Cfg`.
    fn parse_nested(nested_cfg: &NestedMetaItem) -> Result<Cfg, InvalidCfgError> {
        match nested_cfg {
            NestedMetaItem::MetaItem(ref cfg) => Cfg::parse(cfg),
            NestedMetaItem::Literal(ref lit) => Err(InvalidCfgError {
                msg: "unexpected literal",
                span: lit.span,
            }),
        }
    }

    /// Parses a `MetaItem` into a `Cfg`.
    ///
    /// The `MetaItem` should be the content of the `#[cfg(...)]`, e.g., `unix` or
    /// `target_os = "redox"`.
    ///
    /// If the content is not properly formatted, it will return an error indicating what and where
    /// the error is.
    pub fn parse(cfg: &MetaItem) -> Result<Cfg, InvalidCfgError> {
        let name = match cfg.ident() {
            Some(ident) => ident.name,
            None => return Err(InvalidCfgError {
                msg: "expected a single identifier",
                span: cfg.span
            }),
        };
        match cfg.kind {
            MetaItemKind::Word => Ok(Cfg::Cfg(name, None)),
            MetaItemKind::NameValue(ref lit) => match lit.kind {
                LitKind::Str(value, _) => Ok(Cfg::Cfg(name, Some(value))),
                _ => Err(InvalidCfgError {
                    // FIXME: if the main #[cfg] syntax decided to support non-string literals,
                    // this should be changed as well.
                    msg: "value of cfg option should be a string literal",
                    span: lit.span,
                }),
            },
            MetaItemKind::List(ref items) => {
                let mut sub_cfgs = items.iter().map(Cfg::parse_nested);
                match name {
                    sym::all => sub_cfgs.fold(Ok(Cfg::True), |x, y| Ok(x? & y?)),
                    sym::any => sub_cfgs.fold(Ok(Cfg::False), |x, y| Ok(x? | y?)),
                    sym::not => if sub_cfgs.len() == 1 {
                        Ok(!sub_cfgs.next().unwrap()?)
                    } else {
                        Err(InvalidCfgError {
                            msg: "expected 1 cfg-pattern",
                            span: cfg.span,
                        })
                    },
                    _ => Err(InvalidCfgError {
                        msg: "invalid predicate",
                        span: cfg.span,
                    }),
                }
            }
        }
    }

    /// Checks whether the given configuration can be matched in the current session.
    ///
    /// Equivalent to `attr::cfg_matches`.
    // FIXME: Actually make use of `features`.
    pub fn matches(&self, parse_sess: &ParseSess, features: Option<&Features>) -> bool {
        match *self {
            Cfg::False => false,
            Cfg::True => true,
            Cfg::Not(ref child) => !child.matches(parse_sess, features),
            Cfg::All(ref sub_cfgs) => {
                sub_cfgs.iter().all(|sub_cfg| sub_cfg.matches(parse_sess, features))
            },
            Cfg::Any(ref sub_cfgs) => {
                sub_cfgs.iter().any(|sub_cfg| sub_cfg.matches(parse_sess, features))
            },
            Cfg::Cfg(name, value) => parse_sess.config.contains(&(name, value)),
        }
    }

    /// Whether the configuration consists of just `Cfg` or `Not`.
    fn is_simple(&self) -> bool {
        match *self {
            Cfg::False | Cfg::True | Cfg::Cfg(..) | Cfg::Not(..) => true,
            Cfg::All(..) | Cfg::Any(..) => false,
        }
    }

    /// Whether the configuration consists of just `Cfg`, `Not` or `All`.
    fn is_all(&self) -> bool {
        match *self {
            Cfg::False | Cfg::True | Cfg::Cfg(..) | Cfg::Not(..) | Cfg::All(..) => true,
            Cfg::Any(..) => false,
        }
    }

    /// Renders the configuration for human display, as a short HTML description.
    pub(crate) fn render_short_html(&self) -> String {
        let mut msg = Html(self, true).to_string();
        if self.should_capitalize_first_letter() {
            if let Some(i) = msg.find(|c: char| c.is_ascii_alphanumeric()) {
                msg[i .. i+1].make_ascii_uppercase();
            }
        }
        msg
    }

    /// Renders the configuration for long display, as a long HTML description.
    pub(crate) fn render_long_html(&self) -> String {
        let on = if self.should_use_with_in_description() {
            "with"
        } else {
            "on"
        };

        let mut msg = format!("This is supported {} <strong>{}</strong>", on, Html(self, false));
        if self.should_append_only_to_description() {
            msg.push_str(" only");
        }
        msg.push('.');
        msg
    }

    fn should_capitalize_first_letter(&self) -> bool {
        match *self {
            Cfg::False | Cfg::True | Cfg::Not(..) => true,
            Cfg::Any(ref sub_cfgs) | Cfg::All(ref sub_cfgs) => {
                sub_cfgs.first().map(Cfg::should_capitalize_first_letter).unwrap_or(false)
            },
            Cfg::Cfg(name, _) => match &*name.as_str() {
                "debug_assertions" | "target_endian" => true,
                _ => false,
            },
        }
    }

    fn should_append_only_to_description(&self) -> bool {
        match *self {
            Cfg::False | Cfg::True => false,
            Cfg::Any(..) | Cfg::All(..) | Cfg::Cfg(..) => true,
            Cfg::Not(ref child) => match **child {
                Cfg::Cfg(..) => true,
                _ => false,
            }
        }
    }

    fn should_use_with_in_description(&self) -> bool {
        match *self {
            Cfg::Cfg(name, _) if name == sym::target_feature => true,
            _ => false,
        }
    }
}

impl ops::Not for Cfg {
    type Output = Cfg;
    fn not(self) -> Cfg {
        match self {
            Cfg::False => Cfg::True,
            Cfg::True => Cfg::False,
            Cfg::Not(cfg) => *cfg,
            s => Cfg::Not(Box::new(s)),
        }
    }
}

impl ops::BitAndAssign for Cfg {
    fn bitand_assign(&mut self, other: Cfg) {
        match (self, other) {
            (&mut Cfg::False, _) | (_, Cfg::True) => {},
            (s, Cfg::False) => *s = Cfg::False,
            (s @ &mut Cfg::True, b) => *s = b,
            (&mut Cfg::All(ref mut a), Cfg::All(ref mut b)) => a.append(b),
            (&mut Cfg::All(ref mut a), ref mut b) => a.push(mem::replace(b, Cfg::True)),
            (s, Cfg::All(mut a)) => {
                let b = mem::replace(s, Cfg::True);
                a.push(b);
                *s = Cfg::All(a);
            },
            (s, b) => {
                let a = mem::replace(s, Cfg::True);
                *s = Cfg::All(vec![a, b]);
            },
        }
    }
}

impl ops::BitAnd for Cfg {
    type Output = Cfg;
    fn bitand(mut self, other: Cfg) -> Cfg {
        self &= other;
        self
    }
}

impl ops::BitOrAssign for Cfg {
    fn bitor_assign(&mut self, other: Cfg) {
        match (self, other) {
            (&mut Cfg::True, _) | (_, Cfg::False) => {},
            (s, Cfg::True) => *s = Cfg::True,
            (s @ &mut Cfg::False, b) => *s = b,
            (&mut Cfg::Any(ref mut a), Cfg::Any(ref mut b)) => a.append(b),
            (&mut Cfg::Any(ref mut a), ref mut b) => a.push(mem::replace(b, Cfg::True)),
            (s, Cfg::Any(mut a)) => {
                let b = mem::replace(s, Cfg::True);
                a.push(b);
                *s = Cfg::Any(a);
            },
            (s, b) => {
                let a = mem::replace(s, Cfg::True);
                *s = Cfg::Any(vec![a, b]);
            },
        }
    }
}

impl ops::BitOr for Cfg {
    type Output = Cfg;
    fn bitor(mut self, other: Cfg) -> Cfg {
        self |= other;
        self
    }
}

/// Pretty-print wrapper for a `Cfg`. Also indicates whether the "short-form" rendering should be
/// used.
struct Html<'a>(&'a Cfg, bool);

fn write_with_opt_paren<T: fmt::Display>(
    fmt: &mut fmt::Formatter<'_>,
    has_paren: bool,
    obj: T,
) -> fmt::Result {
    if has_paren {
        fmt.write_char('(')?;
    }
    obj.fmt(fmt)?;
    if has_paren {
        fmt.write_char(')')?;
    }
    Ok(())
}


impl<'a> fmt::Display for Html<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self.0 {
            Cfg::Not(ref child) => match **child {
                Cfg::Any(ref sub_cfgs) => {
                    let separator = if sub_cfgs.iter().all(Cfg::is_simple) {
                        " nor "
                    } else {
                        ", nor "
                    };
                    for (i, sub_cfg) in sub_cfgs.iter().enumerate() {
                        fmt.write_str(if i == 0 { "neither " } else { separator })?;
                        write_with_opt_paren(fmt, !sub_cfg.is_all(), Html(sub_cfg, self.1))?;
                    }
                    Ok(())
                }
                ref simple @ Cfg::Cfg(..) => write!(fmt, "non-{}", Html(simple, self.1)),
                ref c => write!(fmt, "not ({})", Html(c, self.1)),
            },

            Cfg::Any(ref sub_cfgs) => {
                let separator = if sub_cfgs.iter().all(Cfg::is_simple) {
                    " or "
                } else {
                    ", or "
                };
                for (i, sub_cfg) in sub_cfgs.iter().enumerate() {
                    if i != 0 {
                        fmt.write_str(separator)?;
                    }
                    write_with_opt_paren(fmt, !sub_cfg.is_all(), Html(sub_cfg, self.1))?;
                }
                Ok(())
            },

            Cfg::All(ref sub_cfgs) => {
                for (i, sub_cfg) in sub_cfgs.iter().enumerate() {
                    if i != 0 {
                        fmt.write_str(" and ")?;
                    }
                    write_with_opt_paren(fmt, !sub_cfg.is_simple(), Html(sub_cfg, self.1))?;
                }
                Ok(())
            },

            Cfg::True => fmt.write_str("everywhere"),
            Cfg::False => fmt.write_str("nowhere"),

            Cfg::Cfg(name, value) => {
                let n = &*name.as_str();
                let human_readable = match (n, value) {
                    ("unix", None) => "Unix",
                    ("windows", None) => "Windows",
                    ("debug_assertions", None) => "debug-assertions enabled",
                    ("target_os", Some(os)) => match &*os.as_str() {
                        "android" => "Android",
                        "dragonfly" => "DragonFly BSD",
                        "emscripten" => "Emscripten",
                        "freebsd" => "FreeBSD",
                        "fuchsia" => "Fuchsia",
                        "haiku" => "Haiku",
                        "hermit" => "HermitCore",
                        "ios" => "iOS",
                        "l4re" => "L4Re",
                        "linux" => "Linux",
                        "macos" => "macOS",
                        "netbsd" => "NetBSD",
                        "openbsd" => "OpenBSD",
                        "redox" => "Redox",
                        "solaris" => "Solaris",
                        "windows" => "Windows",
                        _ => "",
                    },
                    ("target_arch", Some(arch)) => match &*arch.as_str() {
                        "aarch64" => "AArch64",
                        "arm" => "ARM",
                        "asmjs" => "JavaScript",
                        "mips" => "MIPS",
                        "mips64" => "MIPS-64",
                        "msp430" => "MSP430",
                        "powerpc" => "PowerPC",
                        "powerpc64" => "PowerPC-64",
                        "s390x" => "s390x",
                        "sparc64" => "SPARC64",
                        "wasm32" => "WebAssembly",
                        "x86" => "x86",
                        "x86_64" => "x86-64",
                        _ => "",
                    },
                    ("target_vendor", Some(vendor)) => match &*vendor.as_str() {
                        "apple" => "Apple",
                        "pc" => "PC",
                        "rumprun" => "Rumprun",
                        "sun" => "Sun",
                        "fortanix" => "Fortanix",
                        _ => ""
                    },
                    ("target_env", Some(env)) => match &*env.as_str() {
                        "gnu" => "GNU",
                        "msvc" => "MSVC",
                        "musl" => "musl",
                        "newlib" => "Newlib",
                        "uclibc" => "uClibc",
                        "sgx" => "SGX",
                        _ => "",
                    },
                    ("target_endian", Some(endian)) => return write!(fmt, "{}-endian", endian),
                    ("target_pointer_width", Some(bits)) => return write!(fmt, "{}-bit", bits),
                    ("target_feature", Some(feat)) =>
                        if self.1 {
                            return write!(fmt, "<code>{}</code>", feat);
                        } else {
                            return write!(fmt, "target feature <code>{}</code>", feat);
                        },
                    _ => "",
                };
                if !human_readable.is_empty() {
                    fmt.write_str(human_readable)
                } else if let Some(v) = value {
                    write!(fmt, "<code>{}=\"{}\"</code>", Escape(n), Escape(&v.as_str()))
                } else {
                    write!(fmt, "<code>{}</code>", Escape(n))
                }
            }
        }
    }
}
