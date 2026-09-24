//! The DUI parser: a small recursive-descent reader for the component-tree
//! markup. Syntax:
//!
//! ```text
//! // line comments start with //
//! Column #root spacing=8 padding=16 {
//!     Text "Hello, world" size=20
//!     Row spacing=4 align=space-between {
//!         Button #ok "OK" grow=1
//!         Button #cancel "Cancel"
//!     }
//! }
//! ```
//!
//! A node is `Tag (#id)? (attr=value | "text")* ({ children })?`. Container tags
//! (`Row`/`Column`/`Stack`/`Grid`/`Scroll`) lay out their children; any other tag
//! is a sized leaf. Parsing builds a fresh [`Tree`] so the caller can swap it in
//! atomically and keep the previous one as last-known-good on error.

use alloc::string::String;

use crate::attrs::Attrs;
use crate::tree::{Kind, NodeId, Tree, TreeBuilder};
use crate::{Result, UiError};

const MAX_DEPTH: u32 = 128;

/// Parse a DUI document into a retained [`Tree`]. The document must contain
/// exactly one root node (ignoring comments and whitespace).
pub fn parse(src: &str) -> Result<Tree> {
    let mut p = Parser { chars: src.as_bytes(), pos: 0, line: 1 };
    let mut b = TreeBuilder::new();

    p.skip_trivia();
    if p.eof() {
        return Err(UiError::NoRoot);
    }
    let root = p.parse_node(&mut b, None, 0)?;

    p.skip_trivia();
    if !p.eof() {
        return Err(UiError::Unexpected {
            line: p.line,
            what: String::from("trailing content after root node"),
        });
    }
    Ok(b.finish(root))
}

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
    line: u32,
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

    /// Skip whitespace and `//` line comments.
    fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            if c == b'/' && self.chars.get(self.pos + 1) == Some(&b'/') {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.bump();
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
                return Err(UiError::Unexpected {
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

    /// Parse a double-quoted string with `\" \\ \n \t` escapes.
    fn parse_string(&mut self) -> Result<String> {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            let c = match self.bump() {
                Some(c) => c,
                None => return Err(UiError::UnexpectedEof(String::from("unterminated string"))),
            };
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self
                        .bump()
                        .ok_or_else(|| UiError::UnexpectedEof(String::from("dangling escape")))?;
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

    /// Parse an attribute value: a quoted string, or a bareword terminated by
    /// whitespace or a brace.
    fn parse_value(&mut self) -> Result<String> {
        if self.peek() == Some(b'"') {
            return self.parse_string();
        }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || c == b'{' || c == b'}' {
                break;
            }
            self.bump();
        }
        if self.pos == start {
            return Err(UiError::Unexpected {
                line: self.line,
                what: String::from("expected attribute value"),
            });
        }
        Ok(String::from_utf8_lossy(&self.chars[start..self.pos]).into_owned())
    }

    /// Parse one node (and its subtree), append it to `b`, return its id.
    fn parse_node(&mut self, b: &mut TreeBuilder, parent: Option<NodeId>, depth: u32) -> Result<NodeId> {
        if depth > MAX_DEPTH {
            return Err(UiError::Unexpected {
                line: self.line,
                what: String::from("nesting too deep"),
            });
        }
        let tag = self.parse_ident()?;
        let kind = Kind::from_tag(&tag);
        let mut name: Option<String> = None;
        let mut text: Option<String> = None;
        let mut attrs = Attrs::default();
        let mut has_block = false;

        // Inline part: #id, attributes and shorthand text, in any order.
        loop {
            self.skip_trivia();
            match self.peek() {
                Some(b'#') => {
                    self.bump();
                    name = Some(self.parse_ident()?);
                }
                Some(b'"') => {
                    text = Some(self.parse_string()?);
                }
                Some(b'{') => {
                    has_block = true;
                    break;
                }
                Some(c) if Self::is_ident_start(c) => {
                    // Either an attribute (`ident=value`) of this node, or the
                    // tag of a sibling. Look ahead for `=`; rewind if absent.
                    let save_pos = self.pos;
                    let save_line = self.line;
                    let key = self.parse_ident()?;
                    self.skip_trivia();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.skip_trivia();
                        let value = self.parse_value()?;
                        attrs.apply(&key, &value);
                    } else {
                        // Not an attribute — it starts a sibling. Rewind.
                        self.pos = save_pos;
                        self.line = save_line;
                        break;
                    }
                }
                _ => break, // `}`, EOF, or end of this node's inline part
            }
        }

        let id = b.push(name, kind, attrs, text, parent);
        if let Some(p) = parent {
            b.add_child(p, id);
        }

        if has_block {
            self.bump(); // consume '{'
            loop {
                self.skip_trivia();
                match self.peek() {
                    Some(b'}') => {
                        self.bump();
                        break;
                    }
                    None => {
                        return Err(UiError::UnexpectedEof(String::from("unterminated block")))
                    }
                    _ => {
                        self.parse_node(b, Some(id), depth + 1)?;
                    }
                }
            }
        }

        Ok(id)
    }
}
