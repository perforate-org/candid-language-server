use crate::{
    candid_lang::{CandidError, ParserResult, parse},
    lsp::{
        completion::CompletionDocumentCache,
        config::{CompletionEngineMode, ServerConfig, ServiceSnippetStyle},
        navigation::lookup_identifier,
        position::{offset_to_position, position_to_offset, span_to_range},
        semantic_analyze::{Semantic, analyze_program},
        tasks::{DocumentTaskKind, DocumentTaskState, DocumentTaskToken},
    },
};
use candid_parser::{
    candid::{Error as CandidCoreError, error::Label as CandidLabel},
    syntax::IDLMergedProg,
    token::{LexicalError, Token},
};
use dashmap::DashMap;
use lalrpop_util::ParseError;
use rapidhash::fast::RandomState;
use ropey::Rope;
use serde_json::Value;
use std::{
    borrow::Cow,
    error::Error as StdError,
    fmt::Write,
    sync::{Arc, Mutex, RwLock},
};
use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::Result,
    ls_types::{notification::Notification, *},
};

pub mod completion;
pub mod config;
pub mod format;
pub mod hover;
pub mod markdown;
pub mod navigation;
pub mod position;
pub mod semantic_analyze;
pub mod semantic_token;
pub mod span;
pub mod symbol_table;
pub mod tasks;
pub mod type_display;
pub mod type_docs;

use completion::completion as completion_handler;
use format::format as format_handler;
use hover::hover;
use semantic_token::LEGEND_TYPES;

#[derive(Debug)]
pub struct CandidLanguageServer {
    pub client: Client,
    pub documents: DashMap<String, DocumentSnapshot, RandomState>,
    pub analysis_map: DashMap<String, AnalysisSnapshot, RandomState>,
    pub task_states: DashMap<String, Arc<DocumentTaskState>, RandomState>,
    config: RwLock<ServerConfig>,
    hover_offset_cache: Mutex<HoverOffsetCache>,
}

impl LanguageServer for CandidLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(options) = params.initialization_options {
            self.apply_settings_value(options);
        }
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save: None,
                        will_save_wait_until: Some(true),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                    },
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
                document_formatting_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            #[cfg(feature = "proposed")]
            offset_encoding: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "initialized")
            .await;
        let _ = self.refresh_configuration().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let text_document = params.text_document;
        let uri = text_document.uri.clone();
        let uri_label = uri.to_string();
        let version = text_document.version;
        let version_label = Self::version_tag(Some(version));
        self.log_info_event("did_open", format!("uri={} {}", uri_label, version_label))
            .await;
        let text = text_document.text;
        let rope = Rope::from_str(&text);
        self.on_change(TextDocumentItem {
            uri: text_document.uri,
            rope,
            text: Cow::Owned(text),
            version: Some(version),
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
        let version_label = Self::version_tag(version);
        self.log_info_event(
            "did_change",
            format!(
                "uri={} {} changes={}",
                uri_key,
                version_label,
                content_changes.len()
            ),
        )
        .await;
        let (mut rope, current_version) = if let Some(doc) = self.documents.get(&uri_key) {
            (doc.rope().clone(), doc.version())
        } else {
            (Rope::default(), None)
        };

        for change in content_changes {
            let TextDocumentContentChangeEvent { range, text, .. } = change;

            match range {
                None => {
                    rope = Rope::from_str(&text);
                }
                Some(range) => {
                    let mut start_offset = self
                        .cached_position_to_offset(&uri_key, range.start, &rope, current_version)
                        .unwrap_or_else(|| rope.len_chars());
                    let mut end_offset = self
                        .cached_position_to_offset(&uri_key, range.end, &rope, current_version)
                        .unwrap_or(start_offset);

                    let doc_len = rope.len_chars();
                    start_offset = start_offset.min(doc_len);
                    end_offset = end_offset.min(doc_len);

                    if end_offset < start_offset {
                        continue;
                    }

                    rope.remove(start_offset..end_offset);
                    rope.insert(start_offset, &text);
                }
            }
        }

        let text = rope.to_string();

        self.on_change(TextDocumentItem {
            uri,
            rope,
            text: Cow::Owned(text),
            version,
        })
        .await
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let uri_label = uri.to_string();
        let text_provided = params.text.is_some();
        self.log_info_event(
            "did_save",
            format!("uri={} text_included={text_provided}", uri_label),
        )
        .await;
        if let Some(text) = params.text {
            let item = TextDocumentItem {
                uri: params.text_document.uri,
                rope: Rope::from_str(&text),
                text: Cow::Owned(text),
                version: None,
            };
            self.on_change(item).await;
            _ = self.client.semantic_tokens_refresh().await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let uri_label = uri.to_string();
        self.log_info_event("did_close", format!("uri={}", uri_label))
            .await;
        self.task_states.remove(&uri_label);
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.log_info_event("did_change_configuration", "".to_string())
            .await;
        if !self.refresh_configuration().await {
            self.apply_settings_value(params.settings);
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let request_uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let position = params.text_document_position_params.position;
        let request_uri_label = request_uri.to_string();
        self.log_info_event(
            "goto_definition",
            format!(
                "uri={} line={} character={}",
                request_uri_label, position.line, position.character
            ),
        )
        .await;
        let response = (|| {
            let uri = params.text_document_position_params.text_document.uri;
            let uri_key = uri.to_string();
            let analysis = self.analysis_map.get(&uri_key)?;
            let document = self.documents.get(&uri_key)?;
            let semantic = analysis.semantic()?;
            let rope = document.rope();
            let version = document.version();
            let position = params.text_document_position_params.position;
            let offset = self.cached_position_to_offset(&uri_key, position, rope, version)?;

            let info = lookup_identifier(semantic, offset)?;
            let definition_span = info.definition_span?;
            let range = span_to_range(&definition_span, rope)?;

            Some(GotoDefinitionResponse::Scalar(Location::new(uri, range)))
        })();
        self.log_info_event(
            "goto_definition_result",
            format!("uri={} found={}", request_uri_label, response.is_some()),
        )
        .await;

        Ok(response)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let position = params.text_document_position_params.position;
        let uri_label = uri.to_string();
        self.log_info_event(
            "hover",
            format!(
                "uri={} line={} character={}",
                uri_label, position.line, position.character
            ),
        )
        .await;
        let result = hover(self, params).await;
        match &result {
            Ok(Some(_)) => {
                self.log_info_event("hover_result", format!("uri={} found=true", uri_label))
                    .await;
            }
            Ok(None) => {
                self.log_info_event("hover_result", format!("uri={} found=false", uri_label))
                    .await;
            }
            Err(err) => {
                self.log_warn_event("hover_error", format!("uri={} error={err}", uri_label))
                    .await;
            }
        }
        result
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let position = params.text_document_position.position;
        let uri_label = uri.to_string();
        self.log_info_event(
            "completion",
            format!(
                "uri={} line={} character={}",
                uri_label, position.line, position.character
            ),
        )
        .await;
        let result = completion_handler(self, params).await;
        match &result {
            Ok(Some(CompletionResponse::Array(items))) => {
                self.log_info_event(
                    "completion_result",
                    format!("uri={} items={}", uri_label, items.len()),
                )
                .await;
            }
            Ok(Some(_)) => {
                self.log_info_event(
                    "completion_result",
                    format!("uri={} items=unknown", uri_label),
                )
                .await;
            }
            Ok(None) => {
                self.log_info_event("completion_result", format!("uri={} items=0", uri_label))
                    .await;
            }
            Err(err) => {
                self.log_warn_event("completion_error", format!("uri={} error={err}", uri_label))
                    .await;
            }
        }
        result
    }

    async fn will_save_wait_until(
        &self,
        params: WillSaveTextDocumentParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let uri_label = uri.to_string();
        self.log_info_event("will_save_wait_until", format!("uri={}", uri_label))
            .await;

        let options = FormattingOptions {
            tab_size: 2,
            insert_spaces: true,
            ..Default::default()
        };

        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            options,
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
        };

        let result = format_handler(self, params).await;
        match &result {
            Ok(Some(_)) => {
                self.log_info_event(
                    "will_save_wait_until_result",
                    format!("uri={} success=true", uri_label),
                )
                .await;
            }
            Ok(None) => {
                self.log_info_event(
                    "will_save_wait_until_result",
                    format!("uri={} success=false", uri_label),
                )
                .await;
            }
            Err(err) => {
                self.log_warn_event(
                    "will_save_wait_until_error",
                    format!("uri={} error={err}", uri_label),
                )
                .await;
            }
        }
        result
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri.clone();
        let uri_label = uri.to_string();
        self.log_info_event("formatting", format!("uri={}", uri_label))
            .await;
        let result = format_handler(self, params).await;
        match &result {
            Ok(Some(_)) => {
                self.log_info_event(
                    "formatting_result",
                    format!("uri={} success=true", uri_label),
                )
                .await;
            }
            Ok(None) => {
                self.log_info_event(
                    "formatting_result",
                    format!("uri={} success=false", uri_label),
                )
                .await;
            }
            Err(err) => {
                self.log_warn_event("formatting_error", format!("uri={} error={err}", uri_label))
                    .await;
            }
        }
        result
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
    rope: Rope,
    text: Cow<'a, str>,
    version: Option<i32>,
}

#[derive(Debug)]
pub struct DocumentSnapshot {
    rope: Rope,
    version: Option<i32>,
}

impl DocumentSnapshot {
    fn new(rope: Rope, version: Option<i32>) -> Self {
        Self { rope, version }
    }

    fn rope(&self) -> &Rope {
        &self.rope
    }

    fn version(&self) -> Option<i32> {
        self.version
    }
}

#[derive(Debug)]
pub struct AnalysisSnapshot {
    ast: Option<IDLMergedProg>,
    semantic: Option<Semantic>,
    completion_cache: Option<CompletionDocumentCache>,
    parse_errors: usize,
    version: Option<i32>,
}

impl AnalysisSnapshot {
    fn new(
        ast: Option<IDLMergedProg>,
        semantic: Option<Semantic>,
        completion_cache: Option<CompletionDocumentCache>,
        parse_errors: usize,
        version: Option<i32>,
    ) -> Self {
        Self {
            ast,
            semantic,
            completion_cache,
            parse_errors,
            version,
        }
    }

    fn ast(&self) -> Option<&IDLMergedProg> {
        self.ast.as_ref()
    }

    fn semantic(&self) -> Option<&Semantic> {
        self.semantic.as_ref()
    }

    fn completion_cache(&self) -> Option<&CompletionDocumentCache> {
        self.completion_cache.as_ref()
    }

    fn has_parse_errors(&self) -> bool {
        self.parse_errors > 0
    }

    #[allow(dead_code)]
    fn version(&self) -> Option<i32> {
        self.version
    }
}

impl CandidLanguageServer {
    /// Create a new instance of the CandidLanguageServer.
    pub fn new(client: Client) -> Self {
        let hasher = RandomState::new();

        Self {
            client,
            documents: DashMap::with_hasher(hasher),
            analysis_map: DashMap::with_hasher(hasher),
            task_states: DashMap::with_hasher(hasher),
            config: RwLock::new(ServerConfig::default()),
            hover_offset_cache: Mutex::new(HoverOffsetCache::new(64)),
        }
    }

    pub fn service_snippet_style(&self) -> ServiceSnippetStyle {
        let guard = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.service_snippet_style()
    }

    pub fn completion_mode(&self, rope: &Rope) -> CompletionEngineMode {
        let guard = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.completion_mode_for(rope)
    }

    pub fn format_enabled(&self) -> bool {
        let guard = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.format_enabled()
    }

    pub fn format_indent_width(&self) -> Option<usize> {
        let guard = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.format_indent_width()
    }

    pub fn format_blank_lines(&self) -> Option<usize> {
        let guard = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.format_blank_lines()
    }

    pub fn task_token(&self, uri: &str, kind: DocumentTaskKind) -> DocumentTaskToken {
        let state = if let Some(entry) = self.task_states.get(uri) {
            Arc::clone(entry.value())
        } else {
            let state = Arc::new(DocumentTaskState::default());
            self.task_states.insert(uri.to_string(), Arc::clone(&state));
            state
        };
        state.token(kind)
    }

    async fn refresh_configuration(&self) -> bool {
        let items = vec![ConfigurationItem {
            scope_uri: None,
            section: Some("candidLanguageServer".to_string()),
        }];
        match self.client.configuration(items).await {
            Ok(values) => {
                if let Some(value) = values.into_iter().next() {
                    self.apply_settings_value(value);
                    self.log_info_event("configuration", "applied workspace settings".to_string())
                        .await;
                    true
                } else {
                    self.log_info_event(
                        "configuration",
                        "no workspace settings returned".to_string(),
                    )
                    .await;
                    false
                }
            }
            Err(err) => {
                self.log_warn_event(
                    "configuration_error",
                    format!("failed to fetch workspace settings: {err}"),
                )
                .await;
                false
            }
        }
    }

    fn apply_settings_value(&self, value: Value) {
        if value.is_null() {
            return;
        }
        let mut guard = self
            .config
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.apply_settings(value);
    }

    fn version_tag(version: Option<i32>) -> String {
        match version {
            Some(value) => format!("(version: {value})"),
            None => "(version: none)".to_string(),
        }
    }

    fn event_message(event: &str, details: &str) -> String {
        if details.is_empty() {
            format!("[{event}]")
        } else {
            format!("[{event}] {details}")
        }
    }

    async fn log_info_event(&self, event: &str, details: impl Into<String>) {
        let details = details.into();
        let message = Self::event_message(event, &details);
        #[cfg(feature = "tracing")]
        tracing::info!("{message}");
        let _ = self.client.log_message(MessageType::INFO, message).await;
    }

    async fn log_warn_event(&self, event: &str, details: impl Into<String>) {
        let details = details.into();
        let message = Self::event_message(event, &details);
        #[cfg(feature = "tracing")]
        tracing::warn!("{message}");
        let _ = self.client.log_message(MessageType::WARNING, message).await;
    }

    async fn on_change(&self, params: TextDocumentItem<'_>) {
        let TextDocumentItem {
            uri,
            rope,
            text,
            version,
        } = params;
        let uri_key = uri.to_string();
        let version_label = Self::version_tag(version);
        self.log_info_event(
            "document_change",
            format!("uri={} {}", uri_key, version_label),
        )
        .await;

        self.documents.insert(
            uri_key.clone(),
            DocumentSnapshot::new(rope.clone(), version),
        );
        if let Ok(mut cache) = self.hover_offset_cache.lock() {
            cache.invalidate_uri(&uri_key);
        }

        let ParserResult {
            ast, parse_errors, ..
        } = parse(&text);
        let parse_error_count = parse_errors.len();
        self.log_info_event(
            "parse",
            format!("uri={} parse_errors={}", uri_key, parse_error_count),
        )
        .await;

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

        let analysis_snapshot = if let Some(ast) = ast {
            match analyze_program(&ast, &rope) {
                Ok(semantic) => {
                    let completion_cache =
                        CompletionDocumentCache::build(Some(&ast), Some(&semantic), version);
                    self.log_info_event("semantic", format!("uri={} status=ok", uri_key))
                        .await;
                    Some(AnalysisSnapshot::new(
                        Some(ast),
                        Some(semantic),
                        completion_cache,
                        parse_error_count,
                        version,
                    ))
                }
                Err(err) => {
                    let completion_cache =
                        CompletionDocumentCache::build(Some(&ast), None, version);
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
                    self.log_warn_event(
                        "semantic",
                        format!("uri={} status=error error={err}", uri_key),
                    )
                    .await;
                    Some(AnalysisSnapshot::new(
                        Some(ast),
                        None,
                        completion_cache,
                        parse_error_count,
                        version,
                    ))
                }
            }
        } else {
            self.log_info_event("semantic", format!("uri={} status=no-ast", uri_key))
                .await;
            None
        };

        if let Some(snapshot) = analysis_snapshot {
            self.analysis_map.insert(uri_key.clone(), snapshot);
        } else {
            self.analysis_map.remove(&uri_key);
        }

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, version)
            .await;
        self.log_info_event("diagnostics", format!("uri={}", uri_key))
            .await;
    }

    fn cached_position_to_offset(
        &self,
        uri: &str,
        position: Position,
        rope: &Rope,
        version: Option<i32>,
    ) -> Option<usize> {
        {
            if let Ok(mut cache) = self.hover_offset_cache.lock()
                && let Some(offset) = cache.lookup(uri, version, &position)
            {
                return Some(offset);
            }
        }

        let offset = position_to_offset(position, rope)?;

        if let Ok(mut cache) = self.hover_offset_cache.lock() {
            cache.insert(uri.to_string(), version, position, offset);
        }

        Some(offset)
    }
}

#[derive(Debug)]
struct HoverOffsetCache {
    entries: Vec<HoverCacheEntry>,
    capacity: usize,
}

impl HoverOffsetCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
        }
    }

    fn lookup(&mut self, uri: &str, version: Option<i32>, position: &Position) -> Option<usize> {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|entry| entry.matches(uri, version, position))
        {
            let entry = self.entries.remove(idx);
            let offset = entry.offset;
            self.entries.push(entry);
            Some(offset)
        } else {
            None
        }
    }

    fn insert(&mut self, uri: String, version: Option<i32>, position: Position, offset: usize) {
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        self.entries.push(HoverCacheEntry {
            uri,
            version,
            line: position.line,
            column: position.character,
            offset,
        });
    }

    fn invalidate_uri(&mut self, uri: &str) {
        self.entries.retain(|entry| entry.uri != uri);
    }
}

#[derive(Debug)]
struct HoverCacheEntry {
    uri: String,
    version: Option<i32>,
    line: u32,
    column: u32,
    offset: usize,
}

impl HoverCacheEntry {
    fn matches(&self, uri: &str, version: Option<i32>, position: &Position) -> bool {
        self.uri == uri
            && self.version == version
            && self.line == position.line
            && self.column == position.character
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
