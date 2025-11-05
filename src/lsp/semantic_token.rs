use candid_parser::token::Token;
use tower_lsp_server::lsp_types::SemanticTokenType;

// Additional token types used to enrich the semantic-token legend beyond the built-ins.
const COMMENT_DELIMITER: SemanticTokenType = SemanticTokenType::new("commentDelimiter");
const CONSTANT: SemanticTokenType = SemanticTokenType::new("constant");
const IDENTIFIER: SemanticTokenType = SemanticTokenType::new("identifier");
const PUNCTUATION_BRACKET: SemanticTokenType = SemanticTokenType::new("punctuationBracket");
const PUNCTUATION_DELIMITER: SemanticTokenType = SemanticTokenType::new("punctuationDelimiter");
const STRING_DELIMITER: SemanticTokenType = SemanticTokenType::new("stringDelimiter");

// Keep the legend array and index lookup in sync by expanding both from the same list of tokens.

macro_rules! legend_count {
    ($($token:expr),+ $(,)?) => {
        <[()]>::len(&[$({ stringify!($token); }),*])
    };
}

macro_rules! legend_idx_lookup {
    ($needle:expr; $idx:expr; $head:expr, $($tail:expr),+) => {
        if $needle == $head.as_str() {
            $idx
        } else {
            legend_idx_lookup!($needle; $idx + 1usize; $($tail),+)
        }
    };
    ($needle:expr; $idx:expr; $head:expr) => {
        if $needle == $head.as_str() {
            $idx
        } else {
            panic!("legend missing entry for {}", $needle)
        }
    };
}

macro_rules! define_legend {
    ($($token:expr),+ $(,)?) => {
        /// Ordered legend exposed to the LSP client so it can decode semantic tokens we emit.
        pub const LEGEND_TYPES: &[SemanticTokenType; legend_count!($($token),+)] = &[
            $($token),*
        ];

        /// Resolve the legend index for a given semantic-token type, panicking if the legend is stale.
        #[inline]
        fn idx(token_type: &SemanticTokenType) -> usize {
            let needle = token_type.as_str();
            legend_idx_lookup!(needle; 0usize; $($token),+)
        }
    };
}

define_legend!(
    SemanticTokenType::COMMENT,
    COMMENT_DELIMITER,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::TYPE,
    CONSTANT,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    STRING_DELIMITER,
    SemanticTokenType::OPERATOR,
    PUNCTUATION_BRACKET,
    PUNCTUATION_DELIMITER,
    IDENTIFIER,
);

/// Translate a lexed `Token` into the semantic-token index expected by the LSP legend.
#[inline]
pub fn semantic_token_type_index(token: &Token) -> usize {
    match token {
        // Comment
        Token::LineComment => idx(&SemanticTokenType::COMMENT),
        Token::StartComment => idx(&COMMENT_DELIMITER),

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
        | Token::Opt => idx(&SemanticTokenType::KEYWORD),

        // Type
        Token::Blob | Token::Principal => idx(&SemanticTokenType::TYPE),

        // Constant
        Token::Null | Token::Boolean(_) => idx(&CONSTANT),

        // Number
        Token::Decimal(_) | Token::Hex(_) | Token::Float(_) => idx(&SemanticTokenType::NUMBER),

        // String
        Token::Text(_) => idx(&SemanticTokenType::STRING),
        Token::StartString => idx(&STRING_DELIMITER),

        // Operator
        Token::Equals | Token::TestEqual | Token::NotEqual | Token::NotDecode | Token::Sign(_) => {
            idx(&SemanticTokenType::OPERATOR)
        }

        // Punctuation
        Token::LParen | Token::RParen | Token::LBrace | Token::RBrace => idx(&PUNCTUATION_BRACKET),
        Token::Comma | Token::Semi | Token::Colon | Token::Dot | Token::Arrow => {
            idx(&PUNCTUATION_DELIMITER)
        }

        // Identifier
        Token::Id(_) => idx(&IDENTIFIER),
    }
}
