//! Token cursor with backtracking used by the recursive-descent grammar.

use crate::lexer::{Token, TokenKind};

/// A cursor over a token slice. Whitespace tokens stay in place so rules can
/// require or allow them explicitly, matching the source grammar helpers `_`
/// and `__`.
pub struct Stream<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Stream<'a> {
    /// Build a cursor at the start of the tokens.
    pub fn new(tokens: &'a [Token]) -> Self {
        Stream { tokens, pos: 0 }
    }

    /// Current position, for save and restore.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Restore a saved position.
    pub fn set(&mut self, pos: usize) {
        self.pos = pos;
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
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Consume the current token if its kind matches, returning its value.
    pub fn eat(&mut self, kind: &TokenKind) -> Option<String> {
        match self.tokens.get(self.pos) {
            Some(t) if &t.kind == kind => {
                let v = t.value.clone();
                self.pos += 1;
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
                self.pos += 1;
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
            self.pos += 1;
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
