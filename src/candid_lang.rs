use crate::lsp::semantic_token::semantic_token_type_index;
use candid_parser::{
    IDLProg,
    token::{LexicalError, Tokenizer},
};

#[derive(thiserror::Error, Debug)]
pub enum CandidError {
    #[error("Parser error: {0}")]
    Parser(#[from] candid_parser::Error),
    #[error("Lexer error: {0}")]
    Lexer(LexicalError),
}

impl From<LexicalError> for CandidError {
    fn from(err: LexicalError) -> Self {
        CandidError::Lexer(err)
    }
}

/// Incomplete semantic-token entry paired with source offsets produced during lexing.
#[derive(Debug)]
pub struct ImCompleteSemanticToken {
    pub start: usize,
    pub length: usize,
    pub token_type: usize,
}

/// Aggregated parse output containing the AST, errors, and semantic-token data.
#[derive(Debug)]
pub struct ParserResult {
    pub ast: Option<IDLProg>,
    pub parse_errors: Vec<CandidError>,
    pub semantic_tokens: Vec<ImCompleteSemanticToken>,
}

/// Tokenize and parse a Candid source string, returning any partial data collected along the way.
pub fn parse(src: &str) -> ParserResult {
    let mut semantic_tokens = Vec::new();
    let mut parse_errors = Vec::new();
    let tokenizer = Tokenizer::new(src);

    for token in tokenizer {
        match token {
            Ok((start, token, end)) => {
                let token_type = semantic_token_type_index(&token);
                semantic_tokens.push(ImCompleteSemanticToken {
                    start,
                    length: end - start,
                    token_type,
                });
            }
            Err(err) => parse_errors.push(err.into()),
        }
    }

    let ast = match src.parse::<IDLProg>() {
        Ok(ast) => Some(ast),
        Err(err) => {
            // user error duplicates a lexer error
            if !matches!(
                &err,
                candid_parser::Error::Parse(lalrpop_util::ParseError::User { .. })
            ) {
                parse_errors.push(err.into());
            }
            None
        }
    };

    ParserResult {
        ast,
        parse_errors,
        semantic_tokens,
    }
}
