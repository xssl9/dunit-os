//! The DSS parser: a small recursive reader for the bounded stylesheet syntax.
//!
//! ```text
//! // line comments, plus /* block comments */
//! $bg: #101410;              // a variable
//! $fg: #d8e6d8;
//!
//! Button {                   // element selector
//!     color: $fg;
//!     background: $bg;
//!     padding: 6 14;         // a value list
//!     radius: 6;
//!     transition: background 160ms ease-out;
//! }
//! Button:hover { background: #1b241b; }   // state selector
//! .primary { font-weight: bold; }         // class selector
//! #ok, #save { min-width: 96; }           // id list, shared block
//! ```
//!
//! A selector is one compound of an optional element tag followed by any mix of
//! `#id`, `.class` and `:state`. Comma separates selectors that share a block.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use crate::value::Value;
use crate::{Result, StyleError};

/// A parsed stylesheet: an ordered list of rules (variables are expanded away).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

/// One selector paired with its declaration block. `order` is the 0-based
/// position in source, used to break specificity ties (later wins).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
    pub order: u32,
}

/// A single `property: value` pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub property: String,
    pub value: Value,
}

/// A compound selector. Any component may be empty; an all-empty selector is the
/// universal selector and matches every node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    pub element: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub states: Vec<String>,
}

/// Parse a DSS document into a [`Stylesheet`]. Variables (`$name: value;`) are
/// resolved and dropped; each rule keeps its source order for tie-breaking.
pub fn parse(src: &str) -> Result<Stylesheet> {
    let mut p = Parser {
        chars: src.as_bytes(),
        pos: 0,
        line: 1,
        vars: BTreeMap::new(),
    };
    let mut rules = Vec::new();
    let mut order = 0u32;

    loop {
        p.skip_trivia();
        if p.eof() {
            break;
        }
        if p.peek() == Some(b'$') {
            p.parse_var_def()?;
            continue;
        }
        // A selector list sharing one declaration block.
        let selectors = p.parse_selector_list()?;
        let decls = p.parse_block()?;
        for sel in selectors {
            rules.push(Rule { selector: sel, declarations: decls.clone(), order });
            order += 1;
        }
    }
    Ok(Stylesheet { rules })
}

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
    line: u32,
    /// Variable name -> expanded value words.
    vars: BTreeMap<String, Vec<String>>,
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<u8> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Skip whitespace, `//` line comments and `/* */` block comments.
    fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            if c == b'/' && self.chars.get(self.pos + 1) == Some(&b'/') {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.bump();
                }
            } else if c == b'/' && self.chars.get(self.pos + 1) == Some(&b'*') {
                self.bump();
                self.bump();
                while let Some(c) = self.bump() {
                    if c == b'*' && self.peek() == Some(b'/') {
                        self.bump();
                        break;
                    }
                }
            } else if c.is_ascii_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn is_ident_start(c: u8) -> bool {
        c.is_ascii_alphabetic() || c == b'_'
    }

    fn is_ident_char(c: u8) -> bool {
        c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
    }

    fn parse_ident(&mut self) -> Result<String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if Self::is_ident_start(c) => {}
            _ => {
                return Err(StyleError::Unexpected {
                    line: self.line,
                    what: String::from("expected identifier"),
                })
            }
        }
        while let Some(c) = self.peek() {
            if Self::is_ident_char(c) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&self.chars[start..self.pos]).into_owned())
    }
}

impl<'a> Parser<'a> {
    /// Parse `$name: value;` and record the expanded words.
    fn parse_var_def(&mut self) -> Result<()> {
        self.bump(); // '$'
        let name = self.parse_ident()?;
        self.skip_trivia();
        self.expect(b':')?;
        let words = self.read_value_words()?;
        self.expect(b';')?;
        self.vars.insert(name, words);
        Ok(())
    }

    /// Parse one or more comma-separated selectors up to the `{`.
    fn parse_selector_list(&mut self) -> Result<Vec<Selector>> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            out.push(self.parse_selector()?);
            self.skip_trivia();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                }
                Some(b'{') => break,
                _ => {
                    return Err(StyleError::Unexpected {
                        line: self.line,
                        what: String::from("expected ',' or '{' after selector"),
                    })
                }
            }
        }
        Ok(out)
    }

    /// Parse a single compound selector: `element? (#id | .class | :state)*`.
    fn parse_selector(&mut self) -> Result<Selector> {
        let mut sel = Selector::default();
        if let Some(c) = self.peek() {
            if Self::is_ident_start(c) {
                sel.element = Some(self.parse_ident()?);
            }
        }
        loop {
            match self.peek() {
                Some(b'#') => {
                    self.bump();
                    sel.id = Some(self.parse_ident()?);
                }
                Some(b'.') => {
                    self.bump();
                    sel.classes.push(self.parse_ident()?);
                }
                Some(b':') => {
                    self.bump();
                    sel.states.push(self.parse_ident()?);
                }
                _ => break,
            }
        }
        if sel.element.is_none()
            && sel.id.is_none()
            && sel.classes.is_empty()
            && sel.states.is_empty()
        {
            return Err(StyleError::Unexpected {
                line: self.line,
                what: String::from("empty selector"),
            });
        }
        Ok(sel)
    }
}

impl<'a> Parser<'a> {
    /// Parse a `{ declaration* }` block.
    fn parse_block(&mut self) -> Result<Vec<Declaration>> {
        self.skip_trivia();
        self.expect(b'{')?;
        let mut decls = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                Some(b'}') => {
                    self.bump();
                    break;
                }
                None => {
                    return Err(StyleError::UnexpectedEof(String::from("unterminated block")))
                }
                _ => decls.push(self.parse_declaration()?),
            }
        }
        Ok(decls)
    }

    /// Parse `property: value;`.
    fn parse_declaration(&mut self) -> Result<Declaration> {
        let property = self.parse_ident()?;
        self.skip_trivia();
        self.expect(b':')?;
        let words = self.read_value_words()?;
        self.expect(b';')?;
        if words.is_empty() {
            return Err(StyleError::Unexpected {
                line: self.line,
                what: String::from("empty declaration value"),
            });
        }
        Ok(Declaration { property, value: words_to_value(&words) })
    }

    /// Read the value words up to `;` or `}`, expanding `$var` references and
    /// skipping commas (comma- and space-separated lists are treated alike).
    fn read_value_words(&mut self) -> Result<Vec<String>> {
        let mut words = Vec::new();
        loop {
            self.skip_value_ws();
            match self.peek() {
                None => {
                    return Err(StyleError::UnexpectedEof(String::from("unterminated value")))
                }
                Some(b';') | Some(b'}') => break,
                _ => {
                    let w = self.read_word()?;
                    if let Some(name) = w.strip_prefix('$') {
                        if let Some(vals) = self.vars.get(name) {
                            words.extend(vals.iter().cloned());
                        }
                        // An unknown variable expands to nothing.
                    } else {
                        words.push(w);
                    }
                }
            }
        }
        Ok(words)
    }

    /// Skip spaces, tabs, newlines and commas between value words.
    fn skip_value_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || c == b',' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Read one value word: a quoted string, or a run up to whitespace / a
    /// value terminator (`; { } ,`).
    fn read_word(&mut self) -> Result<String> {
        if self.peek() == Some(b'"') {
            return self.parse_string();
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || matches!(c, b';' | b'{' | b'}' | b',') {
                break;
            }
            self.bump();
        }
        Ok(String::from_utf8_lossy(&self.chars[start..self.pos]).into_owned())
    }

    /// Parse a double-quoted string with `\" \\ \n \t` escapes.
    fn parse_string(&mut self) -> Result<String> {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            let c = match self.bump() {
                Some(c) => c,
                None => return Err(StyleError::UnexpectedEof(String::from("unterminated string"))),
            };
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.bump().ok_or_else(|| {
                        StyleError::UnexpectedEof(String::from("dangling escape"))
                    })?;
                    out.push(match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'"' => '"',
                        b'\\' => '\\',
                        other => other as char,
                    });
                }
                other => out.push(other as char),
            }
        }
    }

    fn expect(&mut self, want: u8) -> Result<()> {
        if self.peek() == Some(want) {
            self.bump();
            Ok(())
        } else {
            Err(StyleError::Unexpected {
                line: self.line,
                what: alloc::format!("expected '{}'", want as char),
            })
        }
    }
}

/// Convert value words into a [`Value`]: one word is a scalar, several become a
/// [`Value::List`].
fn words_to_value(words: &[String]) -> Value {
    if words.len() == 1 {
        Value::parse_word(&words[0])
    } else {
        Value::List(words.iter().map(|w| Value::parse_word(w)).collect())
    }
}




