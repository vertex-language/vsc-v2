//! Helpers over the AST.

pub mod classify {
    use crate::ast::{Expr, ExprKind};

    /// Does `e` need no terminator after it when it is a statement? True for
    /// expressions that end with a block (`if`, `match`, loops, blocks).
    pub fn expr_requires_terminator_to_be_stmt(e: &Expr) -> bool {
        !e.is_block_like()
    }

    /// Does `e` end in a struct literal or block, so that in a condition it
    /// would swallow the body's `{`?
    pub fn expr_trailing_brace(e: &Expr) -> bool {
        matches!(e.kind, ExprKind::Struct(..))
    }
}

pub mod parser {
    //! Operator precedence, as in rustc's `util::parser`.

    use crate::ast::BinOpKind;
    use crate::token::TokenKind;

    /// Binary and assignment operators, with what the parser needs to know
    /// about them.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum AssocOp {
        Binary(BinOpKind),
        /// `+=` and friends.
        AssignOp(BinOpKind),
        /// `=`
        Assign,
        /// `as`
        Cast,
        /// `..` and `..=`
        Range(crate::ast::RangeLimits),
    }

    /// Precedence levels, lowest first.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
    pub enum ExprPrecedence {
        /// `return`, `break`, function literals.
        Jump,
        /// `=` and compound assignment: right-associative.
        Assign,
        /// `..` and `..=`
        Range,
        /// `||`
        LOr,
        /// `&&`
        LAnd,
        /// `==` and the other comparisons: they don't chain.
        Compare,
        /// `|`
        BitOr,
        /// `^`
        BitXor,
        /// `&`
        BitAnd,
        /// `<<` and `>>`
        Shift,
        /// `+` and `-`
        Sum,
        /// `*`, `/` and `%`
        Product,
        /// `as`
        Cast,
        /// Unary `-`, `!`, `*`, `&`.
        Prefix,
        /// Postfix and primary expressions.
        Unambiguous,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Fixity {
        Left,
        Right,
        /// Needs parentheses when repeated: `a == b == c` is an error.
        None,
    }

    impl AssocOp {
        pub fn from_token(kind: TokenKind) -> Option<AssocOp> {
            use BinOpKind as B;
            use TokenKind as T;
            Some(match kind {
                T::Eq => AssocOp::Assign,
                T::Plus => AssocOp::Binary(B::Add),
                T::Minus => AssocOp::Binary(B::Sub),
                T::Star => AssocOp::Binary(B::Mul),
                T::Slash => AssocOp::Binary(B::Div),
                T::Percent => AssocOp::Binary(B::Rem),
                T::Caret => AssocOp::Binary(B::BitXor),
                T::And => AssocOp::Binary(B::BitAnd),
                T::Or => AssocOp::Binary(B::BitOr),
                T::Shl => AssocOp::Binary(B::Shl),
                T::Shr => AssocOp::Binary(B::Shr),
                T::PlusEq => AssocOp::AssignOp(B::Add),
                T::MinusEq => AssocOp::AssignOp(B::Sub),
                T::StarEq => AssocOp::AssignOp(B::Mul),
                T::SlashEq => AssocOp::AssignOp(B::Div),
                T::PercentEq => AssocOp::AssignOp(B::Rem),
                T::CaretEq => AssocOp::AssignOp(B::BitXor),
                T::AndEq => AssocOp::AssignOp(B::BitAnd),
                T::OrEq => AssocOp::AssignOp(B::BitOr),
                T::ShlEq => AssocOp::AssignOp(B::Shl),
                T::ShrEq => AssocOp::AssignOp(B::Shr),
                T::Lt => AssocOp::Binary(B::Lt),
                T::Le => AssocOp::Binary(B::Le),
                T::Ge => AssocOp::Binary(B::Ge),
                T::Gt => AssocOp::Binary(B::Gt),
                T::EqEq => AssocOp::Binary(B::Eq),
                T::Ne => AssocOp::Binary(B::Ne),
                T::AndAnd => AssocOp::Binary(B::And),
                T::OrOr => AssocOp::Binary(B::Or),
                T::DotDot => AssocOp::Range(crate::ast::RangeLimits::HalfOpen),
                T::DotDotEq => AssocOp::Range(crate::ast::RangeLimits::Closed),
                T::Ident(name, crate::token::IdentIsRaw::No) if name == vsc_span::kw::As => AssocOp::Cast,
                _ => return None,
            })
        }

        pub fn precedence(self) -> ExprPrecedence {
            use BinOpKind::*;
            match self {
                AssocOp::Cast => ExprPrecedence::Cast,
                AssocOp::Binary(op) => match op {
                    Mul | Div | Rem => ExprPrecedence::Product,
                    Add | Sub => ExprPrecedence::Sum,
                    Shl | Shr => ExprPrecedence::Shift,
                    BitAnd => ExprPrecedence::BitAnd,
                    BitXor => ExprPrecedence::BitXor,
                    BitOr => ExprPrecedence::BitOr,
                    Lt | Gt | Le | Ge | Eq | Ne => ExprPrecedence::Compare,
                    And => ExprPrecedence::LAnd,
                    Or => ExprPrecedence::LOr,
                },
                AssocOp::Range(_) => ExprPrecedence::Range,
                AssocOp::Assign | AssocOp::AssignOp(_) => ExprPrecedence::Assign,
            }
        }

        pub fn fixity(self) -> Fixity {
            match self {
                AssocOp::Assign | AssocOp::AssignOp(_) => Fixity::Right,
                AssocOp::Binary(op) if op.is_comparison() => Fixity::None,
                AssocOp::Range(_) => Fixity::None,
                AssocOp::Binary(_) | AssocOp::Cast => Fixity::Left,
            }
        }
    }
}
