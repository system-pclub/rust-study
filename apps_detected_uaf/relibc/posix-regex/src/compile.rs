//! The regex "compiler", which parses the regex itself.
//! Produces a matcher ready to match input.

#[cfg(feature = "no_std")]
use std::prelude::*;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use {ctype, PosixRegex};

/// Repetition bounds, for example + is (1, None), and ? is (0, Some(1))
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Range(pub u32, pub Option<u32>);
impl fmt::Debug for Range {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Range(start, None) => write!(f, "{}..", start),
            Range(start, Some(end)) => write!(f, "{}..{}", start, end),
        }
    }
}

/// An item inside square brackets, like `[abc]` or `[[:digit:]]`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Collation {
    Char(u8),
    Class(fn(u8) -> bool)
}
impl Collation {
    /// Compare this collation to a character
    pub fn matches(&self, other: u8, insensitive: bool) -> bool {
        match *self {
            Collation::Char(me) if insensitive => me & !32 == other & !32,
            Collation::Char(me) => me == other,
            Collation::Class(f) => f(other)
        }
    }
}

/// A single "compiled" token, such as a `.` or a character literal
#[derive(Clone, PartialEq, Eq)]
pub enum Token {
    InternalStart,

    Any,
    Char(u8),
    End,
    Group {
        id: usize,
        branches: Vec<Vec<(Token, Range)>>
    },
    OneOf {
        invert: bool,
        list: Vec<Collation>
    },
    Start,
    WordEnd,
    WordStart
}
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Token::InternalStart => write!(f, "<START>"),

            Token::Any => write!(f, "."),
            Token::Char(c) => write!(f, "{:?}", c as char),
            Token::End => write!(f, "$"),
            Token::Group { ref branches, .. } => write!(f, "Group({:?})", branches),
            Token::OneOf { invert, ref list } => write!(f, "[invert: {}; {:?}]", invert, list),
            Token::Start => write!(f, "^"),
            Token::WordEnd => write!(f, ">"),
            Token::WordStart => write!(f, "<")
        }
    }
}
/// An error that occurred while compiling the regex
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    EOF,
    EmptyRepetition,
    Expected(u8, Option<u8>),
    IllegalRange,
    IntegerOverflow,
    LeadingRepetition,
    UnclosedRepetition,
    UnexpectedToken(u8),
    UnknownClass(Vec<u8>),
    UnknownCollation
}

/// A regex builder struct
pub struct PosixRegexBuilder<'a> {
    input: &'a [u8],
    classes: HashMap<&'a [u8], fn(u8) -> bool>,
    group_id: usize
}
impl<'a> PosixRegexBuilder<'a> {
    /// Create a new instance that is ready to parse the regex `input`
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            classes: HashMap::new(),
            group_id: 1
        }
    }
    /// Add a custom collation class, for use within square brackets (such as `[[:digit:]]`)
    pub fn with_class(mut self, name: &'a [u8], callback: fn(u8) -> bool) -> Self {
        self.classes.insert(name, callback);
        self
    }
    /// Add all the default collation classes, like `[[:digit:]]` and `[[:alnum:]]`
    pub fn with_default_classes(mut self) -> Self {
        #[cfg(not(feature = "no_std"))]
        self.classes.reserve(12);

        self.classes.insert(b"alnum", ctype::is_alnum);
        self.classes.insert(b"alpha", ctype::is_alpha);
        self.classes.insert(b"blank", ctype::is_blank);
        self.classes.insert(b"cntrl", ctype::is_cntrl);
        self.classes.insert(b"digit", ctype::is_digit);
        self.classes.insert(b"graph", ctype::is_graph);
        self.classes.insert(b"lower", ctype::is_lower);
        self.classes.insert(b"print", ctype::is_print);
        self.classes.insert(b"punct", ctype::is_punct);
        self.classes.insert(b"space", ctype::is_space);
        self.classes.insert(b"upper", ctype::is_upper);
        self.classes.insert(b"xdigit", ctype::is_xdigit);

        self
    }
    /// "Compile" this regex to a struct ready to match input
    pub fn compile(mut self) -> Result<PosixRegex<'static>, Error> {
        let search = self.compile_tokens()?;
        Ok(PosixRegex::new(Cow::Owned(search)))
    }

    fn consume(&mut self, amount: usize) {
        self.input = &self.input[amount..];
    }
    fn take_int(&mut self) -> Result<Option<u32>, Error> {
        let mut out: Option<u32> = None;
        while let Some(&c @ b'0'..=b'9') = self.input.first() {
            self.consume(1);
            out = Some(out.unwrap_or(0)
                .checked_mul(10)
                .and_then(|out| out.checked_add((c - b'0') as u32))
                .ok_or(Error::IntegerOverflow)?);
        }
        Ok(out)
    }
    fn next(&mut self) -> Result<u8, Error> {
        self.input.first()
            .map(|&c| { self.consume(1); c })
            .ok_or(Error::EOF)
    }
    fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.input.first() != Some(&c) {
            return Err(Error::Expected(c, self.input.first().cloned()));
        }
        self.consume(1);
        Ok(())
    }
    pub fn compile_tokens(&mut self) -> Result<Vec<Vec<(Token, Range)>>, Error> {
        let mut alternatives = Vec::new();
        let mut chain: Vec<(Token, Range)> = Vec::new();

        while let Some(&c) = self.input.first() {
            self.consume(1);
            let token = match c {
                b'^' => Token::Start,
                b'$' => Token::End,
                b'.' => Token::Any,
                b'*' => if let Some(last) = chain.last_mut() {
                    last.1 = Range(0, None);
                    continue;
                } else {
                    return Err(Error::LeadingRepetition);
                },
                b'[' => {
                    let mut list = Vec::new();
                    let invert = self.input.first() == Some(&b'^');

                    if invert {
                        self.consume(1);
                    }

                    loop {
                        let mut c = self.next()?;

                        let mut push = true;

                        if c == b'[' {
                            // TODO: Handle collation characters properly,
                            // because currently idk what they are and only
                            // have the behavior of `grep` to go on.
                            match self.next()? {
                                b'.' => {
                                    c = self.next()?;
                                    self.expect(b'.')?;
                                    self.expect(b']')?;
                                },
                                b'=' => {
                                    c = self.next()?;
                                    self.expect(b'=')?;
                                    self.expect(b']')?;
                                },
                                b':' => {
                                    let end = self.input.iter().position(|&c| c == b':').ok_or(Error::EOF)?;
                                    let key = &self.input[..end];
                                    let class = *self.classes.get(key).ok_or_else(|| Error::UnknownClass(key.to_vec()))?;
                                    self.consume(end + 1);
                                    self.expect(b']')?;

                                    list.push(Collation::Class(class));
                                    push = false;
                                },
                                _ => return Err(Error::UnknownCollation)
                            }
                        }

                        if push {
                            list.push(Collation::Char(c));

                            if self.input.first() == Some(&b'-') && self.input.get(1) != Some(&b']') {
                                self.consume(1);
                                let dest = self.next()?;
                                for c in (c+1)..=dest {
                                    list.push(Collation::Char(c));
                                }
                            }
                        }

                        if self.input.first() == Some(&b']') {
                            self.consume(1);
                            break;
                        }
                    }

                    Token::OneOf {
                        invert,
                        list
                    }
                },
                b'\\' => match self.next()? {
                    b'(' => {
                        let id = self.group_id;
                        self.group_id += 1;
                        Token::Group {
                            id,
                            branches: self.compile_tokens()?
                        }
                    },
                    b')' => {
                        alternatives.push(chain);
                        return Ok(alternatives);
                    }
                    b'|' => {
                        alternatives.push(chain);
                        chain = Vec::new();
                        continue;
                    },
                    b'<' => Token::WordStart,
                    b'>' => Token::WordEnd,
                    c@b'?' | c@b'+' => if let Some(last) = chain.last_mut() {
                        last.1 = match c {
                            b'?' => Range(0, Some(1)),
                            b'+' => Range(1, None),
                            _ => unreachable!(c)
                        };
                        continue;
                    } else {
                        return Err(Error::LeadingRepetition);
                    },
                    b'{' => if let Some(last) = chain.last_mut() {
                        let first = self.take_int()?.ok_or(Error::EmptyRepetition)?;
                        let mut second = Some(first);
                        if let Some(b',') = self.input.first() {
                            self.consume(1);
                            second = self.take_int()?;
                        }
                        if self.input.first() == Some(&b'}') {
                            self.consume(1);
                        } else if self.input.starts_with(br"\}") {
                            self.consume(2);
                        } else {
                            return Err(Error::UnclosedRepetition);
                        }
                        if second.map(|second| first > second).unwrap_or(false) {
                            return Err(Error::IllegalRange);
                        }
                        last.1 = Range(first, second);
                        continue;
                    } else {
                        return Err(Error::LeadingRepetition);
                    },
                    b'a' => Token::OneOf { invert: false, list: vec![Collation::Class(ctype::is_alnum)] },
                    b'd' => Token::OneOf { invert: false, list: vec![Collation::Class(ctype::is_digit)] },
                    b's' => Token::OneOf { invert: false, list: vec![Collation::Class(ctype::is_space)] },
                    b'S' => Token::OneOf { invert: true,  list: vec![Collation::Class(ctype::is_space)] },
                    b'n' => Token::Char(b'\n'),
                    b'r' => Token::Char(b'\r'),
                    b't' => Token::Char(b'\t'),
                    c => Token::Char(c)
                },
                c => Token::Char(c)
            };
            chain.push((token, Range(1, Some(1))));
        }

        alternatives.push(chain);
        Ok(alternatives)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(input: &[u8]) -> Vec<(Token, Range)> {
        PosixRegexBuilder::new(input)
            .with_default_classes()
            .compile_tokens()
            .expect("error compiling regex")
            .into_iter()
            .next()
            .unwrap()
    }
    fn t(t: Token) -> (Token, Range) {
        (t, Range(1, Some(1)))
    }
    fn c(c: u8) -> (Token, Range) {
        t(Token::Char(c))
    }

    #[test]
    fn basic() {
        assert_eq!(compile(b"abc"), &[c(b'a'), c(b'b'), c(b'c')]);
    }
    #[test]
    fn groups() {
        assert_eq!(compile(br"\(abc\|bcd\|cde\)"), &[t(Token::Group { id: 1, branches: vec![
            vec![c(b'a'), c(b'b'), c(b'c')],
            vec![c(b'b'), c(b'c'), c(b'd')],
            vec![c(b'c'), c(b'd'), c(b'e')]
        ]})]);
        assert_eq!(compile(br"\(abc\|\(bcd\|cde\)\)"), &[
            t(Token::Group { id: 1, branches: vec![
                vec![c(b'a'), c(b'b'), c(b'c')],
                vec![t(Token::Group { id: 2, branches: vec![
                    vec![c(b'b'), c(b'c'), c(b'd')],
                    vec![c(b'c'), c(b'd'), c(b'e')]
                ]})]
            ]})
        ]);
    }
    #[test]
    fn words() {
        assert_eq!(
            compile(br"\<word\>"),
            &[t(Token::WordStart), c(b'w'), c(b'o'), c(b'r'), c(b'd'), t(Token::WordEnd)]
        );
    }
    #[test]
    fn repetitions() {
        assert_eq!(
            compile(br"yeee*"),
            &[c(b'y'), c(b'e'), c(b'e'), (Token::Char(b'e'), Range(0, None))]
        );
        assert_eq!(
            compile(br"yee\?"),
            &[c(b'y'), c(b'e'), (Token::Char(b'e'), Range(0, Some(1)))]
        );
        assert_eq!(
            compile(br"yee\+"),
            &[c(b'y'), c(b'e'), (Token::Char(b'e'), Range(1, None))]
        );
        assert_eq!(
            compile(br"ye\{2}"),
            &[c(b'y'), (Token::Char(b'e'), Range(2, Some(2)))]
        );
        assert_eq!(
            compile(br"ye\{2,}"),
            &[c(b'y'), (Token::Char(b'e'), Range(2, None))]
        );
        assert_eq!(
            compile(br"ye\{2,3}"),
            &[c(b'y'), (Token::Char(b'e'), Range(2, Some(3)))]
        );
    }
    #[test]
    fn bracket() {
        assert_eq!(
            compile(b"[abc]"),
            &[t(Token::OneOf {
                invert: false,
                list: vec![
                    Collation::Char(b'a'),
                    Collation::Char(b'b'),
                    Collation::Char(b'c')
                ]
            })]
        );
        assert_eq!(
            compile(b"[^abc]"),
            &[t(Token::OneOf {
                invert: true,
                list: vec![
                    Collation::Char(b'a'),
                    Collation::Char(b'b'),
                    Collation::Char(b'c')
                ]
            })]
        );
        assert_eq!(
            compile(b"[]] [^]]"),
            &[
                t(Token::OneOf { invert: false, list: vec![ Collation::Char(b']') ] }),
                c(b' '),
                t(Token::OneOf { invert: true,  list: vec![ Collation::Char(b']') ] }),
            ]
        );
        assert_eq!(
            compile(b"[0-3] [a-c] [-1] [1-]"),
            &[
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Char(b'0'),
                    Collation::Char(b'1'),
                    Collation::Char(b'2'),
                    Collation::Char(b'3')
                ] }),
                c(b' '),
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Char(b'a'),
                    Collation::Char(b'b'),
                    Collation::Char(b'c')
                ] }),
                c(b' '),
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Char(b'-'),
                    Collation::Char(b'1')
                ] }),
                c(b' '),
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Char(b'1'),
                    Collation::Char(b'-')
                ] })
            ]
        );
        assert_eq!(
            compile(b"[[.-.]-/]"),
            &[
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Char(b'-'),
                    Collation::Char(b'.'),
                    Collation::Char(b'/')
                ] })
            ]
        );
        assert_eq!(
            compile(b"[[:digit:][:upper:]]"),
            &[
                t(Token::OneOf { invert: false, list: vec![
                    Collation::Class(ctype::is_digit),
                    Collation::Class(ctype::is_upper)
                ] })
            ]
        );
    }
    #[test]
    fn newline() {
        assert_eq!(
            compile(br"\r\n"),
            &[c(b'\r'), c(b'\n')]
        );
    }
}
