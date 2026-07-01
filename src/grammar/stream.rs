//! Token cursor with backtracking used by the recursive-descent grammar.

use crate::lexer::{Token, TokenKind};

/// A cursor over a token slice. Whitespace tokens stay in place so rules can
/// require or allow them explicitly, matching the source grammar helpers `_`
/// and `__`.
///
/// The cursor also keeps a high-water mark: the furthest position any rule
/// reached, even after backtracking. Error reporting uses it to name the token
/// where parsing broke rather than the statement start it rewound to.
pub struct Stream<'a> {
    tokens: &'a [Token],
    pos: usize,
    furthest: usize,
}

impl<'a> Stream<'a> {
    /// Build a cursor at the start of the tokens.
    pub fn new(tokens: &'a [Token]) -> Self {
        Stream {
            tokens,
            pos: 0,
            furthest: 0,
        }
    }

    /// Current position, for save and restore.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The furthest position reached, across all attempts including ones that
    /// backtracked. This is the token where parsing broke.
    pub fn furthest(&self) -> usize {
        self.furthest
    }

    /// Restore a saved position. Does not lower the high-water mark.
    pub fn set(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Advance the cursor and raise the high-water mark if needed.
    fn advance_to(&mut self, pos: usize) {
        self.pos = pos;
        if pos > self.furthest {
            self.furthest = pos;
        }
    }

    /// Whether the cursor is at end of input.
    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Peek at the current token without consuming.
    pub fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Peek at the token at an absolute position.
    pub fn at(&self, pos: usize) -> Option<&Token> {
        self.tokens.get(pos)
    }

    /// Consume the current token.
    pub fn bump(&mut self) -> Option<&Token> {
        if self.pos < self.tokens.len() {
            self.advance_to(self.pos + 1);
            self.tokens.get(self.pos - 1)
        } else {
            None
        }
    }

    /// Consume the current token if its kind matches, returning its value.
    pub fn eat(&mut self, kind: &TokenKind) -> Option<String> {
        match self.tokens.get(self.pos) {
            Some(t) if &t.kind == kind => {
                let v = t.value.clone();
                self.advance_to(self.pos + 1);
                Some(v)
            }
            _ => None,
        }
    }

    /// Consume a keyword by name, returning its raw value.
    pub fn eat_keyword(&mut self, name: &str) -> Option<String> {
        match self.tokens.get(self.pos) {
            Some(Token {
                kind: TokenKind::Keyword(k),
                value,
            }) if k == name => {
                let v = value.clone();
                self.advance_to(self.pos + 1);
                Some(v)
            }
            _ => None,
        }
    }

    /// Consume zero or more whitespace tokens. Always succeeds. This is `_`.
    pub fn ws0(&mut self) {
        while let Some(Token {
            kind: TokenKind::Ws,
            ..
        }) = self.tokens.get(self.pos)
        {
            self.advance_to(self.pos + 1);
        }
    }

    /// Consume one or more whitespace tokens. This is `__`. Returns false if
    /// none were present.
    pub fn ws1(&mut self) -> bool {
        let start = self.pos;
        self.ws0();
        self.pos > start
    }
}
