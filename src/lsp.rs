use crate::{
    candid_lang::{CandidError, ImCompleteSemanticToken, ParserResult, parse},
    lsp::{
        navigation::lookup_identifier,
        position::{offset_to_position, position_to_offset, span_to_range},
        semantic_analyze::{Semantic, analyze_program},
    },
};
use candid_parser::{
    IDLProg,
    candid::{Error as CandidCoreError, error::Label as CandidLabel},
    token::{LexicalError, Token},
};
use dashmap::DashMap;
use lalrpop_util::ParseError;
use log::debug;
use rapidhash::fast::RandomState;
use ropey::Rope;
use std::{error::Error as StdError, fmt::Write};
use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::Result,
    lsp_types::{notification::Notification, *},
};

pub mod hover;
pub mod navigation;
pub mod position;
pub mod semantic_analyze;
pub mod semantic_token;
pub mod span;
pub mod symbol_table;

use hover::hover;
use semantic_token::LEGEND_TYPES;

#[derive(Debug)]
pub struct CandidLanguageServer {
    pub client: Client,
    pub ast_map: DashMap<String, IDLProg, RandomState>,
    pub semantic_map: DashMap<String, Semantic, RandomState>,
    pub semantic_token_map: DashMap<String, Vec<ImCompleteSemanticToken>, RandomState>,
    pub document_map: DashMap<String, Rope, RandomState>,
}

impl LanguageServer for CandidLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["dummy.do_something".to_string()],
                    ..Default::default()
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions {
                                work_done_progress: Some(false),
                            },
                            legend: SemanticTokensLegend {
                                token_types: LEGEND_TYPES.to_vec(),
                                token_modifiers: vec![],
                            },
                            range: Some(true),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            #[cfg(feature = "proposed")]
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        debug!("file opened");
        self.on_change(&TextDocumentItem {
            uri: params.text_document.uri,
            text: &params.text_document.text,
            version: Some(params.text_document.version),
        })
        .await
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let DidChangeTextDocumentParams {
            text_document,
            content_changes,
        } = params;

        let uri = text_document.uri;
        let version = Some(text_document.version);
        let uri_key = uri.to_string();
        let mut rope = self
            .document_map
            .get(&uri_key)
            .map(|doc| doc.value().clone())
            .unwrap_or_default();

        for change in content_changes {
            if change.range.is_none() {
                rope = Rope::from_str(&change.text);
                continue;
            }

            let Some(range) = change.range else {
                continue;
            };

            let mut start_offset =
                position_to_offset(range.start, &rope).unwrap_or_else(|| rope.len_chars());
            let mut end_offset = position_to_offset(range.end, &rope).unwrap_or(start_offset);

            let doc_len = rope.len_chars();
            start_offset = start_offset.min(doc_len);
            end_offset = end_offset.min(doc_len);

            if end_offset < start_offset {
                continue;
            }

            rope.remove(start_offset..end_offset);
            rope.insert(start_offset, &change.text);
        }

        let text = rope.to_string();

        self.on_change(&TextDocumentItem {
            uri,
            text: &text,
            version,
        })
        .await
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        dbg!(&params.text);
        if let Some(text) = params.text {
            let item = TextDocumentItem {
                uri: params.text_document.uri,
                text: &text,
                version: None,
            };
            self.on_change(&item).await;
            _ = self.client.semantic_tokens_refresh().await;
        }
        debug!("file saved!");
    }

    async fn did_close(&self, _: DidCloseTextDocumentParams) {
        debug!("file closed!");
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let response = (|| {
            let uri = params.text_document_position_params.text_document.uri;
            let uri_key = uri.to_string();
            let semantic = self.semantic_map.get(&uri_key)?;
            let rope = self.document_map.get(&uri_key)?;
            let position = params.text_document_position_params.position;
            let offset = position_to_offset(position, &rope)?;

            let info = lookup_identifier(&semantic, offset)?;
            let definition_span = info.definition_span?;
            let range = span_to_range(&definition_span, &rope)?;

            Some(GotoDefinitionResponse::Scalar(Location::new(uri, range)))
        })();

        Ok(response)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        hover(self, params)
    }
}

#[allow(unused)]
enum CustomNotification {}
impl Notification for CustomNotification {
    type Params = InlayHintParams;
    const METHOD: &'static str = "custom/notification";
}

struct TextDocumentItem<'a> {
    uri: Uri,
    text: &'a str,
    version: Option<i32>,
}

impl<'a> CandidLanguageServer {
    async fn on_change(&self, params: &TextDocumentItem<'a>) {
        let uri_key = params.uri.to_string();
        let rope = ropey::Rope::from_str(params.text);

        self.document_map.insert(uri_key.clone(), rope.clone());

        let ParserResult {
            ast,
            parse_errors,
            semantic_tokens,
        } = parse(params.text);

        let mut diagnostics = Vec::with_capacity(parse_errors.len());
        for item in parse_errors {
            let diag = match item {
                CandidError::Parser(err) => match err {
                    candid_parser::Error::Parse(parse_err) => {
                        Some(parse_error_to_diagnostic(parse_err, &rope))
                    }
                    candid_parser::Error::Custom(err) => {
                        let mut message = String::from("custom parser error: ");
                        message.push_str(&format_error_chain(err.as_ref()));
                        Some(Diagnostic {
                            range: Range::default(),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("parser".to_string()),
                            message,
                            related_information: None,
                            ..Default::default()
                        })
                    }
                    candid_parser::Error::CandidError(err) => {
                        Some(candid_error_to_diagnostic(&err, &rope))
                    }
                },
                CandidError::Lexer(err) => {
                    let start_position = offset_to_position(err.span.start, &rope);
                    let end_position = offset_to_position(err.span.end, &rope);
                    match (start_position, end_position) {
                        (Some(start), Some(end)) => Some(Diagnostic {
                            range: Range::new(start, end),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("lexer".to_string()),
                            message: err.to_string(),
                            related_information: None,
                            ..Default::default()
                        }),
                        _ => None,
                    }
                }
            };

            if let Some(mut diag) = diag {
                diag.message = clean_diagnostic_message(diag.message);
                diagnostics.push(diag);
            }
        }

        if let Some(ast) = ast {
            match analyze_program(&ast) {
                Ok(semantic) => {
                    self.semantic_map.insert(uri_key.clone(), semantic);
                }
                Err(err) => {
                    self.semantic_map.remove(&uri_key);
                    let span = err.span();
                    let start_position = offset_to_position(span.start, &rope);
                    let end_position = offset_to_position(span.end, &rope);
                    if let (Some(start), Some(end)) = (start_position, end_position) {
                        let diag = Diagnostic::new_simple(
                            Range::new(start, end),
                            clean_diagnostic_message(format!("{err}")),
                        );
                        diagnostics.push(diag);
                    } else {
                        let diag = Diagnostic {
                            range: Range::default(),
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("semantic".to_string()),
                            message: clean_diagnostic_message(format!("{err}")),
                            related_information: None,
                            ..Default::default()
                        };
                        diagnostics.push(diag);
                    }
                }
            }
            self.ast_map.insert(uri_key.clone(), ast);
        } else {
            self.ast_map.remove(&uri_key);
            self.semantic_map.remove(&uri_key);
        }

        self.client
            .publish_diagnostics(params.uri.clone(), diagnostics, params.version)
            .await;
        self.semantic_token_map.insert(uri_key, semantic_tokens);
    }
}

fn range_single_char(offset: usize, rope: &Rope) -> Range {
    if let Some(start) = offset_to_position(offset, rope) {
        let end_offset = offset.saturating_add(1);
        let end = offset_to_position(end_offset, rope)
            .unwrap_or_else(|| Position::new(start.line, start.character + 1));
        Range::new(start, end)
    } else {
        Range::default()
    }
}

fn range_offsets(start_offset: usize, end_offset: usize, rope: &Rope) -> Range {
    match (
        offset_to_position(start_offset, rope),
        offset_to_position(end_offset, rope),
    ) {
        (Some(start), Some(end)) => Range::new(start, end),
        _ => Range::default(),
    }
}

fn format_error_chain(err: &(dyn StdError + 'static)) -> String {
    let mut message = String::new();
    let mut current: Option<&(dyn StdError + 'static)> = Some(err);
    let mut depth = 0;

    while let Some(source) = current {
        if depth == 0 {
            let _ = write!(message, "{}", source);
        } else {
            let _ = write!(message, "\nCaused by ({depth}): {}", source);
        }
        depth += 1;
        current = source.source();
    }

    clean_diagnostic_message(message)
}

fn clean_diagnostic_message(mut message: String) -> String {
    const SUFFIX_PREFIX: &str = " at ";

    if let Some(idx) = message.rfind(SUFFIX_PREFIX) {
        let suffix = message[idx + SUFFIX_PREFIX.len()..].trim();
        if let Some((start, end)) = suffix.split_once("..") {
            let is_digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            if is_digits(start) && is_digits(end) {
                message.truncate(idx);
                message = message.trim_end().to_string();
            }
        }
    }

    message
}

fn candid_error_to_diagnostic(err: &CandidCoreError, rope: &Rope) -> Diagnostic {
    use CandidCoreError::{Binread, Custom, Reserve, Subtype};

    match err {
        Binread(labels) => {
            let mut message = err.to_string();
            if let Some(label) = labels.first() {
                let range = binread_label_range(label, rope).unwrap_or_default();
                let extras = labels
                    .iter()
                    .skip(1)
                    .filter_map(binread_label_message)
                    .collect::<Vec<_>>();

                if !extras.is_empty() {
                    message.push_str("\nAdditional details:");
                    for extra in extras {
                        message.push_str("\n  - ");
                        message.push_str(&extra);
                    }
                }
                message = clean_diagnostic_message(message);

                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("candid".to_string()),
                    message,
                    related_information: None,
                    ..Default::default()
                }
            } else {
                message = clean_diagnostic_message(message);
                Diagnostic {
                    message,
                    ..candid_error_with_fallback(err)
                }
            }
        }
        Subtype(_) | Custom(_) | Reserve(_) => candid_error_with_fallback(err),
    }
}

fn candid_error_with_fallback(err: &CandidCoreError) -> Diagnostic {
    Diagnostic {
        range: Range::default(),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("candid".to_string()),
        message: format_error_chain(err),
        related_information: None,
        ..Default::default()
    }
}

fn binread_label_range(label: &CandidLabel, rope: &Rope) -> Option<Range> {
    let debug = format!("{label:?}");
    let pos_part = debug.split("pos: ").nth(1)?.split(',').next()?;
    let pos = pos_part.trim().parse::<usize>().ok()?;
    let start_offset = pos / 2;
    offset_to_position(start_offset, rope).map(|start| {
        let end = Position::new(start.line, start.character + 1);
        Range::new(start, end)
    })
}

fn binread_label_message(label: &CandidLabel) -> Option<String> {
    let debug = format!("{label:?}");
    let rest = debug.split("message: \"").nth(1)?;
    let msg = rest.split('\"').next()?.replace("\\\"", "\"");
    Some(msg)
}

pub fn parse_error_to_diagnostic(
    err: ParseError<usize, Token, LexicalError>,
    rope: &Rope,
) -> Diagnostic {
    match err {
        ParseError::InvalidToken { location } => Diagnostic {
            range: range_single_char(location, rope),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("parser".to_string()),
            message: clean_diagnostic_message("invalid token".to_string()),
            ..Default::default()
        },
        ParseError::UnrecognizedEof { location, expected } => Diagnostic {
            range: range_single_char(location, rope),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("parser".to_string()),
            message: clean_diagnostic_message(format!(
                "unexpected end of file, expected one of: {}",
                expected.join(", ")
            )),
            ..Default::default()
        },
        ParseError::UnrecognizedToken { token, expected } => {
            let (start, _tok, end) = token;
            Diagnostic {
                range: range_offsets(start, end, rope),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("parser".to_string()),
                message: clean_diagnostic_message(format!(
                    "unexpected token, expected one of: {}",
                    expected.join(", ")
                )),
                ..Default::default()
            }
        }
        ParseError::ExtraToken { token } => {
            let (start, _tok, end) = token;
            Diagnostic {
                range: range_offsets(start, end, rope),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("parser".to_string()),
                message: clean_diagnostic_message("extra token".to_string()),
                ..Default::default()
            }
        }
        ParseError::User { error } => Diagnostic {
            range: Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("parser".to_string()),
            message: clean_diagnostic_message(error.to_string()),
            ..Default::default()
        },
    }
}
