use candid_parser::token::Token;
use tower_lsp_server::lsp_types::SemanticTokenType;

// Additional token types used to enrich the semantic-token legend beyond the built-ins.
const COMMENT_DELIMITER: SemanticTokenType = SemanticTokenType::new("commentDelimiter");
const CONSTANT: SemanticTokenType = SemanticTokenType::new("constant");
const IDENTIFIER: SemanticTokenType = SemanticTokenType::new("identifier");
const PUNCTUATION_BRACKET: SemanticTokenType = SemanticTokenType::new("punctuationBracket");
const PUNCTUATION_DELIMITER: SemanticTokenType = SemanticTokenType::new("punctuationDelimiter");
const STRING_DELIMITER: SemanticTokenType = SemanticTokenType::new("stringDelimiter");

macro_rules! legend_count {
    ($($token:expr),+ $(,)?) => {
        <[()]>::len(&[$({ stringify!($token); }),*])
    };
}

/// Keep the enum, legend order, and match arms synchronized via a single macro invocation.
macro_rules! define_legend {
    ($($variant:ident => $token:expr),+ $(,)?) => {
        #[repr(usize)]
        #[derive(Copy, Clone)]
        enum LegendIdx {
            $( $variant, )+
        }

        impl LegendIdx {
            #[inline]
            const fn idx(self) -> usize {
                self as usize
            }
        }

        /// Ordered legend exposed to the LSP client so it can decode semantic tokens we emit.
        pub const LEGEND_TYPES: &[SemanticTokenType; legend_count!($($token),+)] = &[
            $($token),*
        ];

        #[inline]
        fn idx(kind: LegendIdx) -> usize {
            kind.idx()
        }
    };
}

define_legend!(
    Comment => SemanticTokenType::COMMENT,
    CommentDelimiter => COMMENT_DELIMITER,
    Keyword => SemanticTokenType::KEYWORD,
    Type => SemanticTokenType::TYPE,
    Constant => CONSTANT,
    Number => SemanticTokenType::NUMBER,
    String => SemanticTokenType::STRING,
    StringDelimiter => STRING_DELIMITER,
    Operator => SemanticTokenType::OPERATOR,
    PunctuationBracket => PUNCTUATION_BRACKET,
    PunctuationDelimiter => PUNCTUATION_DELIMITER,
    Identifier => IDENTIFIER,
);

/// Translate a lexed `Token` into the semantic-token index expected by the LSP legend.
#[inline]
pub fn semantic_token_type_index(token: &Token) -> usize {
    match token {
        // Comment
        Token::LineComment => idx(LegendIdx::Comment),
        Token::StartComment => idx(LegendIdx::CommentDelimiter),

        // Keyword
        Token::Vec
        | Token::Record
        | Token::Variant
        | Token::Func
        | Token::Service
        | Token::Oneway
        | Token::Query
        | Token::CompositeQuery
        | Token::Type
        | Token::Import
        | Token::Opt => idx(LegendIdx::Keyword),

        // Type
        Token::Blob | Token::Principal => idx(LegendIdx::Type),

        // Constant
        Token::Null | Token::Boolean(_) => idx(LegendIdx::Constant),

        // Number
        Token::Decimal(_) | Token::Hex(_) | Token::Float(_) => idx(LegendIdx::Number),

        // String
        Token::Text(_) => idx(LegendIdx::String),
        Token::StartString => idx(LegendIdx::StringDelimiter),

        // Operator
        Token::Equals | Token::TestEqual | Token::NotEqual | Token::NotDecode | Token::Sign(_) => {
            idx(LegendIdx::Operator)
        }

        // Punctuation
        Token::LParen | Token::RParen | Token::LBrace | Token::RBrace => {
            idx(LegendIdx::PunctuationBracket)
        }
        Token::Comma | Token::Semi | Token::Colon | Token::Dot | Token::Arrow => {
            idx(LegendIdx::PunctuationDelimiter)
        }

        // Identifier
        Token::Id(_) => idx(LegendIdx::Identifier),
    }
}
