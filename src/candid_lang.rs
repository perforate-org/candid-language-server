use crate::lsp::semantic_token::semantic_token_type_index;
use candid_parser::{
    syntax::spanned::IDLProg,
    token::{LexicalError, Token, Tokenizer, TriviaMap},
};
use lalrpop_util::ParseError;

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
    let trivia = TriviaMap::default();
    let mut tokenizer = RecordingTokenizer::new(src, trivia.clone());
    let (ast, parser_errors) = IDLProg::parse_lossy_from_tokens(Some(&trivia), &mut tokenizer);

    let (semantic_tokens, lexer_errors) = tokenizer.into_parts();
    let mut parse_errors: Vec<CandidError> = lexer_errors
        .iter()
        .cloned()
        .map(CandidError::Lexer)
        .collect();

    for err in parser_errors {
        if let ParseError::User { error } = &err
            && lexer_errors.iter().any(|recorded| recorded == error)
        {
            continue;
        }

        parse_errors.push(CandidError::Parser(candid_parser::Error::Parse(err)));
    }

    ParserResult {
        ast,
        parse_errors,
        semantic_tokens,
    }
}

struct RecordingTokenizer<'src> {
    inner: Tokenizer<'src>,
    semantic_tokens: Vec<ImCompleteSemanticToken>,
    lexer_errors: Vec<LexicalError>,
}

impl<'src> RecordingTokenizer<'src> {
    fn new(src: &'src str, trivia: TriviaMap) -> Self {
        Self {
            inner: Tokenizer::new_with_trivia(src, trivia),
            semantic_tokens: Vec::new(),
            lexer_errors: Vec::new(),
        }
    }

    fn into_parts(self) -> (Vec<ImCompleteSemanticToken>, Vec<LexicalError>) {
        (self.semantic_tokens, self.lexer_errors)
    }
}

impl<'src> Iterator for RecordingTokenizer<'src> {
    type Item = Result<(usize, Token, usize), LexicalError>;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.inner.next()?;

        match &token {
            Ok((start, token, end)) => {
                let token_type = semantic_token_type_index(token);
                self.semantic_tokens.push(ImCompleteSemanticToken {
                    start: *start,
                    length: end - start,
                    token_type,
                });
            }
            Err(err) => self.lexer_errors.push(err.clone()),
        }

        Some(token)
    }
}
