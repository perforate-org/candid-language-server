use crate::lsp::{
    CandidLanguageServer,
    config::{CompletionEngineMode, ServiceSnippetStyle},
    markdown,
    position::position_to_offset,
    semantic_analyze::{
        MethodSignature, ParamRole, Semantic, compute_binding_ident_span, compute_field_label_span,
    },
    span::Span,
    symbol_table::{ImportEntry, ImportKind, SymbolId},
    tasks::{DocumentTaskKind, DocumentTaskToken},
    type_docs::{
        KeywordDoc, TypeDoc, blob_doc, keyword_doc, keyword_kinds, primitive_doc, primitive_kinds,
        primitive_name,
    },
};
use candid_parser::{
    candid::types::internal::FuncMode,
    syntax::{Dec, IDLMergedProg, IDLType, IDLTypeWithSpan},
};
use once_cell::sync::OnceCell;
use rapidhash::fast::RandomState;
use ropey::Rope;
#[cfg(feature = "tracing")]
use std::time::Instant;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};
use tower_lsp_server::{
    jsonrpc::Result,
    ls_types::{
        CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Documentation,
        InsertTextFormat, MarkupContent, MarkupKind,
    },
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CompletionContext {
    Unknown,
    Type,
    Value,
    Definition,
    TopLevel,
    Comment,
}

#[derive(Clone, Debug, Default)]
pub struct ContextSpans {
    type_spans: Vec<Span>,
    value_spans: Vec<Span>,
    definition_spans: Vec<Span>,
}

#[derive(Debug, Default)]
pub struct CompletionDocumentCache {
    spans: OnceCell<Option<ContextSpans>>,
    scope_index: OnceCell<Option<ScopeBindingIndex>>,
    has_ast: bool,
    has_semantic: bool,
    version: Option<i32>,
}

#[cfg(feature = "tracing")]
struct PhaseTimer<'a> {
    uri: &'a str,
    phase: &'static str,
    start: Instant,
}

#[cfg(feature = "tracing")]
impl<'a> PhaseTimer<'a> {
    fn new(uri: &'a str, phase: &'static str) -> Self {
        tracing::trace!(target = "completion", uri = uri, phase = phase, "start");
        Self {
            uri,
            phase,
            start: Instant::now(),
        }
    }
}

#[derive(Debug)]
enum CompletionBuildOutcome {
    Completed(Vec<CompletionItem>),
    Cancelled,
}

struct BuildCompletionItemsParams<'a> {
    context: CompletionContext,
    semantic_ref: Option<&'a Semantic>,
    rope: &'a Rope,
    snippet_style: ServiceSnippetStyle,
    scope_index: Option<&'a ScopeBindingIndex>,
    offset: Option<usize>,
    inside_service_block: bool,
    cursor_context: Option<&'a CursorContext>,
    lightweight: bool,
}

impl<'a> BuildCompletionItemsParams<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        context: CompletionContext,
        semantic_ref: Option<&'a Semantic>,
        rope: &'a Rope,
        snippet_style: ServiceSnippetStyle,
        scope_index: Option<&'a ScopeBindingIndex>,
        offset: Option<usize>,
        inside_service_block: bool,
        cursor_context: Option<&'a CursorContext>,
        lightweight: bool,
    ) -> Self {
        Self {
            context,
            semantic_ref,
            rope,
            snippet_style,
            scope_index,
            offset,
            inside_service_block,
            cursor_context,
            lightweight,
        }
    }
}

#[cfg(feature = "tracing")]
impl<'a> Drop for PhaseTimer<'a> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        tracing::debug!(
            target = "completion",
            uri = self.uri,
            phase = self.phase,
            elapsed_us = elapsed.as_micros()
        );
    }
}

impl CompletionDocumentCache {
    pub fn build(
        ast: Option<&IDLMergedProg>,
        semantic: Option<&Semantic>,
        version: Option<i32>,
    ) -> Option<Self> {
        let has_ast = ast.is_some();
        let has_semantic = semantic.is_some();
        if !has_ast && !has_semantic {
            None
        } else {
            Some(Self {
                spans: OnceCell::new(),
                scope_index: OnceCell::new(),
                has_ast,
                has_semantic,
                version,
            })
        }
    }

    fn spans<'a>(
        &'a self,
        version: Option<i32>,
        ast: Option<&'a IDLMergedProg>,
        semantic: Option<&'a Semantic>,
        rope: Option<&'a Rope>,
    ) -> Option<&'a ContextSpans> {
        if !self.has_ast && !self.has_semantic {
            return None;
        }
        if !self.is_fresh(version) {
            return None;
        }
        self.spans
            .get_or_init(|| ContextSpans::from_sources(ast, semantic, rope))
            .as_ref()
    }

    fn scope_index<'a>(
        &'a self,
        semantic: Option<&'a Semantic>,
        version: Option<i32>,
    ) -> Option<&'a ScopeBindingIndex> {
        if !self.has_semantic {
            return None;
        }
        if !self.is_fresh(version) {
            return None;
        }
        self.scope_index
            .get_or_init(|| semantic.and_then(ScopeBindingIndex::from_semantic))
            .as_ref()
    }

    fn is_fresh(&self, version: Option<i32>) -> bool {
        match (self.version, version) {
            (Some(expected), Some(actual)) => expected == actual,
            (Some(_), None) => false,
            _ => true,
        }
    }
}

impl ContextSpans {
    pub fn from_sources(
        ast: Option<&IDLMergedProg>,
        semantic: Option<&Semantic>,
        rope: Option<&Rope>,
    ) -> Option<Self> {
        let mut spans = ContextSpans::default();
        if let Some(semantic) = semantic {
            spans.collect_from_semantic(semantic);
        }
        if let Some(ast) = ast {
            spans.collect_from_ast(ast, rope);
        }
        if spans.is_empty() { None } else { Some(spans) }
    }

    fn classify(&self, offset: usize) -> CompletionContext {
        if spans_contain(&self.definition_spans, offset) {
            CompletionContext::Definition
        } else if spans_contain(&self.value_spans, offset) {
            CompletionContext::Value
        } else if spans_contain(&self.type_spans, offset) {
            CompletionContext::Type
        } else {
            CompletionContext::Unknown
        }
    }

    fn collect_from_semantic(&mut self, semantic: &Semantic) {
        for field in semantic.fields.iter() {
            if let Some(span) = &field.type_span {
                self.add_type_span(span);
            }
            if let Some(span) = &field.label_span {
                self.add_value_span(span);
            }
        }
        for method in semantic.service_methods.iter() {
            if let Some(span) = &method.type_span {
                self.add_type_span(span);
            }
            if let Some(span) = &method.name_span {
                self.add_value_span(span);
            }
        }
        for param in semantic.params.iter() {
            self.add_type_span(&param.type_span);
            if let Some(span) = &param.name_span {
                self.add_value_span(span);
            }
        }
        for span in semantic
            .symbol_ident_spans
            .iter()
            .filter_map(|span| span.clone())
        {
            self.add_definition_span(&span);
        }
        for reference in semantic.table.reference_id_to_reference.iter() {
            self.add_type_span(&reference.span);
        }
        for import in semantic.table.imports.iter() {
            self.add_definition_span(&import.span);
        }
        for local in semantic.locals.iter() {
            if local.is_definition {
                self.add_definition_span(&local.span);
            }
        }
        for (span, _) in semantic.primitive_spans.iter() {
            self.add_type_span(span);
        }
        for (span, _) in semantic.keyword_spans.iter() {
            self.add_type_span(span);
        }
        if let Some(actor) = &semantic.actor {
            self.add_type_span(&actor.span);
            if let Some(name_span) = &actor.name_span {
                self.add_definition_span(name_span);
                self.add_value_span(name_span);
            }
        }
    }

    fn collect_from_ast(&mut self, ast: &IDLMergedProg, rope: Option<&Rope>) {
        for dec in ast.decs().iter() {
            match dec {
                Dec::TypD(binding) => {
                    if let Some(rope) = rope
                        && let Some(span) = compute_binding_ident_span(binding, rope)
                    {
                        self.add_definition_span(&span);
                    }
                    self.collect_type_from(&binding.typ, rope);
                }
                Dec::ImportType { span, .. } | Dec::ImportServ { span, .. } => {
                    self.add_definition_span(span);
                }
            }
        }
        if let Some(actor) = &ast.resolve_actor().ok().flatten() {
            self.collect_type_from(&actor.typ, rope);
            self.add_type_span(&actor.span);
        }
    }

    fn collect_type_from(&mut self, idl_type: &IDLTypeWithSpan, rope: Option<&Rope>) {
        self.add_type_span(&idl_type.span);
        match &idl_type.kind {
            IDLType::OptT(inner) | IDLType::VecT(inner) => self.collect_type_from(inner, rope),
            IDLType::RecordT(fields) | IDLType::VariantT(fields) => {
                for field in fields {
                    if let Some(rope) = rope
                        && let Some(span) = compute_field_label_span(field, rope)
                    {
                        self.add_value_span(&span);
                    }
                    self.collect_type_from(&field.typ, rope);
                }
            }
            IDLType::ServT(bindings) => {
                for binding in bindings {
                    if let Some(rope) = rope
                        && let Some(span) = compute_binding_ident_span(binding, rope)
                    {
                        self.add_value_span(&span);
                    }
                    self.collect_type_from(&binding.typ, rope);
                }
            }
            IDLType::FuncT(func) => {
                for arg in func.args.iter() {
                    self.collect_type_from(arg, rope);
                }
                for ret in func.rets.iter() {
                    self.collect_type_from(ret, rope);
                }
            }
            IDLType::ClassT(args, ret) => {
                for arg in args.iter() {
                    self.collect_type_from(arg, rope);
                }
                self.collect_type_from(ret, rope);
            }
            IDLType::PrimT(_) | IDLType::VarT(_) | IDLType::PrincipalT => {}
        }
    }

    fn add_type_span(&mut self, span: &Span) {
        if span.start < span.end {
            self.type_spans.push(span.clone());
        }
    }

    fn add_value_span(&mut self, span: &Span) {
        if span.start < span.end {
            self.value_spans.push(span.clone());
        }
    }

    fn add_definition_span(&mut self, span: &Span) {
        if span.start < span.end {
            self.definition_spans.push(span.clone());
        }
    }

    fn is_empty(&self) -> bool {
        self.type_spans.is_empty()
            && self.value_spans.is_empty()
            && self.definition_spans.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
struct ScopeBindingIndex {
    scopes: Vec<ScopeBindings>,
}

impl ScopeBindingIndex {
    fn from_semantic(semantic: &Semantic) -> Option<Self> {
        if semantic.locals.is_empty() {
            return None;
        }
        let mut grouped: BTreeMap<(usize, usize), ScopeBindings> = BTreeMap::new();
        for local in semantic.locals.iter() {
            let key = (local.scope.start, local.scope.end);
            let entry = grouped.entry(key).or_insert_with(|| ScopeBindings {
                span: local.scope.clone(),
                locals: Vec::new(),
            });
            entry.locals.push(LocalBindingEntry {
                name: local.name.clone(),
            });
        }
        Some(ScopeBindingIndex {
            scopes: grouped.into_values().collect(),
        })
    }

    fn bindings_at(&self, offset: usize) -> Option<&[LocalBindingEntry]> {
        self.scopes
            .iter()
            .filter(|scope| span_contains(&scope.span, offset))
            .min_by_key(|scope| span_len(&scope.span))
            .map(|scope| scope.locals.as_slice())
    }
}

#[derive(Clone, Debug)]
struct ScopeBindings {
    span: Span,
    locals: Vec<LocalBindingEntry>,
}

#[derive(Clone, Debug)]
struct LocalBindingEntry {
    name: Arc<str>,
}

#[derive(Default)]
struct FieldGroup {
    parents: BTreeSet<Option<Arc<str>>>,
    docs: Option<Arc<str>>,
}

pub async fn completion(
    server: &CandidLanguageServer,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let uri_key = uri.to_string();

    let (rope, doc_version) = if let Some(doc) = server.documents.get(&uri_key) {
        (doc.rope().clone(), doc.version())
    } else {
        (Rope::default(), None)
    };
    let offset = position_to_offset(position, &rope);
    let cursor_context = offset.map(|offset| CursorContext::new(&rope, offset));

    let completion_mode = server.completion_mode(&rope);
    let is_lightweight = matches!(completion_mode, CompletionEngineMode::Lightweight);

    let analysis_guard = server.analysis_map.get(&uri_key);
    let (semantic_ref, ast_ref, cached_context) = match analysis_guard.as_ref() {
        Some(analysis) => {
            let cache = analysis.completion_cache();
            (
                analysis.semantic(),
                analysis.ast(),
                cache.filter(|cache| cache.is_fresh(doc_version)),
            )
        }
        None => (None, None, None),
    };
    let scope_index = cached_context.and_then(|cache| cache.scope_index(semantic_ref, doc_version));

    let context = {
        #[cfg(feature = "tracing")]
        let _context_timer = PhaseTimer::new(&uri_key, "determine_context");
        determine_context(
            offset,
            cached_context,
            semantic_ref,
            ast_ref,
            &rope,
            cursor_context.as_ref(),
            doc_version,
        )
    };

    #[cfg(feature = "tracing")]
    tracing::debug!(
        target = "completion",
        uri = %uri_key,
        ?context,
        mode = ?completion_mode,
        inside_service_block = cursor_context
            .as_ref()
            .map(|ctx| ctx.inside_service_block())
            .unwrap_or(false)
    );

    let snippet_style = server.service_snippet_style();

    let inside_service_block = cursor_context
        .as_ref()
        .map(|ctx| ctx.inside_service_block())
        .unwrap_or(false);

    let token = server.task_token(&uri_key, DocumentTaskKind::Completion);

    let items = {
        #[cfg(feature = "tracing")]
        let _build_timer = PhaseTimer::new(&uri_key, "build_completion_items");
        match build_completion_items_async(
            BuildCompletionItemsParams::new(
                context,
                semantic_ref,
                &rope,
                snippet_style,
                scope_index,
                offset,
                inside_service_block,
                cursor_context.as_ref(),
                is_lightweight,
            ),
            token,
        )
        .await
        {
            CompletionBuildOutcome::Completed(items) => items,
            CompletionBuildOutcome::Cancelled => {
                #[cfg(feature = "tracing")]
                tracing::debug!(target = "completion", uri = %uri_key, "cancelled");
                Vec::new()
            }
        }
    };

    #[cfg(feature = "tracing")]
    tracing::debug!(
        target = "completion",
        uri = %uri_key,
        completion_items = items.len()
    );

    Ok(Some(CompletionResponse::Array(items)))
}

fn static_completion_items() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    items.extend(keyword_completion_items());
    items.extend(primitive_completion_items());
    items.push(blob_completion_item());
    items
}

fn keyword_completion_items() -> Vec<CompletionItem> {
    keyword_kinds()
        .iter()
        .filter(|kind| **kind != KeywordDoc::Import)
        .filter_map(|kind| {
            keyword_doc(*kind).map(|docs| build_keyword_completion_item(*kind, docs))
        })
        .collect()
}

fn top_level_completion_items() -> Vec<CompletionItem> {
    [KeywordDoc::Type, KeywordDoc::Service, KeywordDoc::Import]
        .into_iter()
        .filter_map(|kind| keyword_doc(kind).map(|docs| build_keyword_completion_item(kind, docs)))
        .collect()
}

#[cfg_attr(not(any(test, feature = "bench")), allow(dead_code))]
fn build_completion_items_sync(params: BuildCompletionItemsParams<'_>) -> Vec<CompletionItem> {
    let BuildCompletionItemsParams {
        context,
        semantic_ref,
        rope,
        snippet_style,
        scope_index,
        offset,
        inside_service_block,
        cursor_context,
        lightweight,
    } = params;
    let mut items = Vec::new();
    let mut seen: HashSet<(String, Option<String>), RandomState> =
        HashSet::with_hasher(RandomState::new());

    match context {
        CompletionContext::Value => {
            if let Some(offset) = offset {
                let value_items = if lightweight {
                    lightweight_value_completion_items(semantic_ref, rope, offset, scope_index)
                } else if let Some(semantic) = semantic_ref {
                    value_completion_items(semantic, rope, offset, snippet_style, scope_index)
                } else {
                    Vec::new()
                };
                append_sorted_items(&mut items, &mut seen, value_items);
            }
        }
        CompletionContext::Definition => {
            if let Some(offset_chars) = offset
                && inside_service_block
            {
                let value_items = if lightweight {
                    lightweight_value_completion_items(
                        semantic_ref,
                        rope,
                        offset_chars,
                        scope_index,
                    )
                } else if let Some(semantic) = semantic_ref {
                    value_completion_items(semantic, rope, offset_chars, snippet_style, scope_index)
                } else {
                    Vec::new()
                };
                append_sorted_items(&mut items, &mut seen, value_items);
            } else {
                return Vec::new();
            }
        }
        CompletionContext::TopLevel => {
            for item in top_level_completion_items() {
                push_if_unique(&mut items, &mut seen, item);
            }
        }
        CompletionContext::Comment => {
            return Vec::new();
        }
        _ => {
            for item in static_completion_items() {
                push_if_unique(&mut items, &mut seen, item);
            }

            if !lightweight && let Some(semantic) = semantic_ref {
                let dynamic_items = semantic_completion_items(semantic, rope);
                append_sorted_items(&mut items, &mut seen, dynamic_items);
            }
        }
    }

    if !matches!(
        context,
        CompletionContext::Value | CompletionContext::Definition
    ) && let (Some(semantic), Some(offset_chars)) = (semantic_ref, offset)
        && inside_service_block
    {
        let service_items = service_method_items(
            semantic,
            rope,
            offset_chars,
            snippet_style,
            cursor_context,
            lightweight,
        );
        append_sorted_items(&mut items, &mut seen, service_items);
    }

    items
}

async fn build_completion_items_async(
    params: BuildCompletionItemsParams<'_>,
    token: DocumentTaskToken,
) -> CompletionBuildOutcome {
    let BuildCompletionItemsParams {
        context,
        semantic_ref,
        rope,
        snippet_style,
        scope_index,
        offset,
        inside_service_block,
        cursor_context,
        lightweight,
    } = params;
    if token.yield_and_check().await.is_err() {
        return CompletionBuildOutcome::Cancelled;
    }
    let mut items = Vec::new();
    let mut seen: HashSet<(String, Option<String>), RandomState> =
        HashSet::with_hasher(RandomState::new());

    match context {
        CompletionContext::Value => {
            if let Some(offset) = offset {
                let value_items = if lightweight {
                    lightweight_value_completion_items(semantic_ref, rope, offset, scope_index)
                } else if let Some(semantic) = semantic_ref {
                    value_completion_items(semantic, rope, offset, snippet_style, scope_index)
                } else {
                    Vec::new()
                };
                append_sorted_items(&mut items, &mut seen, value_items);
            }
        }
        CompletionContext::Definition => {
            if let Some(offset_chars) = offset
                && inside_service_block
            {
                let value_items = if lightweight {
                    lightweight_value_completion_items(
                        semantic_ref,
                        rope,
                        offset_chars,
                        scope_index,
                    )
                } else if let Some(semantic) = semantic_ref {
                    value_completion_items(semantic, rope, offset_chars, snippet_style, scope_index)
                } else {
                    Vec::new()
                };
                append_sorted_items(&mut items, &mut seen, value_items);
            } else {
                return CompletionBuildOutcome::Completed(Vec::new());
            }
        }
        CompletionContext::TopLevel => {
            for item in top_level_completion_items() {
                push_if_unique(&mut items, &mut seen, item);
            }
        }
        CompletionContext::Comment => {
            return CompletionBuildOutcome::Completed(Vec::new());
        }
        _ => {
            for item in static_completion_items() {
                push_if_unique(&mut items, &mut seen, item);
            }

            if !lightweight && let Some(semantic) = semantic_ref {
                let dynamic_items = semantic_completion_items(semantic, rope);
                append_sorted_items(&mut items, &mut seen, dynamic_items);
            }
        }
    }

    if token.yield_and_check().await.is_err() {
        return CompletionBuildOutcome::Cancelled;
    }

    if !matches!(
        context,
        CompletionContext::Value | CompletionContext::Definition
    ) && let (Some(semantic), Some(offset_chars)) = (semantic_ref, offset)
        && inside_service_block
    {
        let service_items = service_method_items(
            semantic,
            rope,
            offset_chars,
            snippet_style,
            cursor_context,
            lightweight,
        );
        append_sorted_items(&mut items, &mut seen, service_items);
    }

    if token.yield_and_check().await.is_err() {
        return CompletionBuildOutcome::Cancelled;
    }

    CompletionBuildOutcome::Completed(items)
}

fn determine_context(
    offset: Option<usize>,
    cached: Option<&CompletionDocumentCache>,
    semantic: Option<&Semantic>,
    ast: Option<&IDLMergedProg>,
    rope: &Rope,
    cursor_context: Option<&CursorContext>,
    version: Option<i32>,
) -> CompletionContext {
    let Some(offset) = offset else {
        return CompletionContext::Unknown;
    };
    let mut derived_cursor_context = None;
    let cursor_context = cursor_context.unwrap_or_else(|| {
        derived_cursor_context = Some(CursorContext::new(rope, offset));
        derived_cursor_context.as_ref().unwrap()
    });

    let heuristic = heuristic_context_from_cursor(cursor_context);
    if matches!(
        heuristic,
        CompletionContext::Value
            | CompletionContext::TopLevel
            | CompletionContext::Definition
            | CompletionContext::Comment
    ) {
        return heuristic;
    }

    if let Some(spans) = cached.and_then(|cache| cache.spans(version, ast, semantic, Some(rope))) {
        let ctx = spans.classify(offset);
        if ctx != CompletionContext::Unknown {
            return ctx;
        }
    }

    if let Some(ctx) =
        ContextSpans::from_sources(ast, semantic, Some(rope)).map(|spans| spans.classify(offset))
        && ctx != CompletionContext::Unknown
    {
        return ctx;
    }

    if heuristic != CompletionContext::Unknown {
        heuristic
    } else {
        CompletionContext::Unknown
    }
}
#[cfg(test)]
fn heuristic_context(offset: Option<usize>, rope: &Rope) -> CompletionContext {
    let Some(offset) = offset else {
        return CompletionContext::Unknown;
    };
    let cursor_context = CursorContext::new(rope, offset);
    heuristic_context_from_cursor(&cursor_context)
}

fn heuristic_context_from_cursor(cursor_context: &CursorContext) -> CompletionContext {
    if cursor_context.rope_len == 0 {
        return CompletionContext::TopLevel;
    }
    if cursor_context.clamped < cursor_context.line_start {
        return CompletionContext::Unknown;
    }
    let trimmed = cursor_context.trimmed_line.as_str();
    let cursor_state = &cursor_context.cursor_state;
    let in_value_block =
        cursor_context.inside_record_variant_block || cursor_context.inside_service_block;
    if cursor_state.in_comment() {
        return CompletionContext::Comment;
    }
    if trimmed.is_empty() {
        if in_value_block {
            return CompletionContext::Value;
        }
        return if cursor_state.is_top_level() {
            CompletionContext::TopLevel
        } else {
            CompletionContext::Unknown
        };
    }
    if let Some(eq_idx) = trimmed.rfind('=') {
        let before = trimmed[..eq_idx].trim_end();
        if before.trim_start().starts_with("type") {
            return CompletionContext::Type;
        }
    }
    if let Some(colon_idx) = trimmed.rfind(':') {
        let before = trimmed[..colon_idx].trim_end();
        if !before.ends_with("service") {
            return CompletionContext::Type;
        }
    }
    if !trimmed.contains(':') && in_value_block {
        return CompletionContext::Value;
    }
    let stripped = trimmed.trim_start();
    if stripped.starts_with("type") && !trimmed.contains('=') {
        return CompletionContext::Definition;
    }
    if cursor_state.is_top_level() {
        return CompletionContext::TopLevel;
    }
    CompletionContext::Unknown
}

#[derive(Default)]
struct CursorScanState {
    depth: usize,
    in_line_comment: bool,
    in_block_comment: bool,
    in_string: bool,
}

impl CursorScanState {
    fn is_top_level(&self) -> bool {
        self.depth == 0 && !self.in_block_comment && !self.in_string && !self.in_line_comment
    }

    fn in_comment(&self) -> bool {
        self.in_block_comment || self.in_line_comment
    }
}

struct CursorContext {
    clamped: usize,
    rope_len: usize,
    line_start: usize,
    trimmed_line: String,
    cursor_state: CursorScanState,
    inside_record_variant_block: bool,
    inside_service_block: bool,
}

impl CursorContext {
    fn new(rope: &Rope, offset: usize) -> Self {
        let rope_len = rope.len_chars();
        let clamped = offset.min(rope_len);
        let total_lines = rope.len_lines().max(1);
        let last_line_index = total_lines.saturating_sub(1);
        let line_index = if rope_len == 0 {
            0
        } else if clamped == rope_len {
            last_line_index
        } else {
            rope.char_to_line(clamped)
        };
        let capped_index = line_index.min(last_line_index);
        let line_start = rope.line_to_char(capped_index);
        let line_prefix = if clamped >= line_start {
            rope.slice(line_start..clamped).to_string()
        } else {
            String::new()
        };
        let trimmed_line = line_prefix.trim_end().to_string();
        let prefix = if clamped == 0 {
            String::new()
        } else {
            rope.slice(0..clamped).to_string()
        };
        let cursor_state = cursor_scan_state_from_text(&prefix);
        let inside_record_variant_block = inside_record_variant_block_from_text(&prefix);
        let inside_service_block = inside_service_block_from_text(&prefix);
        Self {
            clamped,
            rope_len,
            line_start,
            trimmed_line,
            cursor_state,
            inside_record_variant_block,
            inside_service_block,
        }
    }

    fn inside_service_block(&self) -> bool {
        self.inside_service_block
    }
}

fn cursor_scan_state_from_text(text: &str) -> CursorScanState {
    if text.is_empty() {
        return CursorScanState::default();
    }
    let mut chars = text.chars().peekable();
    let mut state = CursorScanState::default();
    let mut escape = false;
    while let Some(ch) = chars.next() {
        if state.in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                state.in_string = false;
            }
            continue;
        }
        if state.in_line_comment {
            if ch == '\n' {
                state.in_line_comment = false;
            }
            continue;
        }
        if state.in_block_comment {
            if ch == '*' && matches!(chars.peek(), Some('/')) {
                chars.next();
                state.in_block_comment = false;
            }
            continue;
        }
        match ch {
            '"' => state.in_string = true,
            '/' => match chars.peek() {
                Some('/') => {
                    chars.next();
                    state.in_line_comment = true;
                }
                Some('*') => {
                    chars.next();
                    state.in_block_comment = true;
                }
                _ => {}
            },
            '{' => state.depth += 1,
            '}' => {
                if state.depth > 0 {
                    state.depth -= 1;
                }
            }
            _ => {}
        }
    }
    state
}

fn inside_record_variant_block_from_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let Some(open_pos) = text.rfind('{') else {
        return false;
    };
    if text[open_pos + 1..].contains('}') {
        return false;
    }
    let prefix = text[..open_pos].trim_end();
    prefix.ends_with("record") || prefix.ends_with("variant")
}

fn inside_service_block_from_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let Some(open_pos) = text.rfind('{') else {
        return false;
    };
    if text[open_pos + 1..].contains('}') {
        return false;
    }
    let prefix = text[..open_pos].trim_end();
    let Some(colon_pos) = prefix.rfind(':') else {
        return false;
    };
    if !prefix[colon_pos + 1..].trim().is_empty() {
        return false;
    }
    let before_colon = prefix[..colon_pos].trim_end();
    contains_keyword(before_colon, "service")
}

fn inside_service_block(rope: &Rope, offset: usize) -> bool {
    if offset == 0 {
        return false;
    }
    let text = rope.slice(0..offset).to_string();
    inside_service_block_from_text(&text)
}

fn contains_keyword(text: &str, keyword: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative) = text[search_start..].find(keyword) {
        let absolute = search_start + relative;
        let before_ok = text[..absolute]
            .chars()
            .last()
            .map(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            .unwrap_or(true);
        let after_index = absolute + keyword.len();
        let after_ok = text[after_index..]
            .chars()
            .next()
            .map(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        search_start = absolute + keyword.len();
    }
    false
}

fn primitive_completion_items() -> impl Iterator<Item = CompletionItem> {
    primitive_kinds().iter().cloned().map(|kind| {
        build_completion_item(
            primitive_name(&kind),
            CompletionItemKind::STRUCT,
            Some("primitive type"),
            Some(primitive_doc(&kind)),
        )
    })
}

fn blob_completion_item() -> CompletionItem {
    build_completion_item(
        "blob",
        CompletionItemKind::STRUCT,
        Some("type alias"),
        Some(blob_doc()),
    )
}

fn build_completion_item(
    label: &str,
    kind: CompletionItemKind,
    detail: Option<&str>,
    docs: Option<&str>,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: detail.map(|text| text.to_string()),
        documentation: docs.map(|value| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: value.to_string(),
            })
        }),
        ..CompletionItem::default()
    }
}

fn build_keyword_completion_item(kind: KeywordDoc, docs: &str) -> CompletionItem {
    let mut item = build_completion_item(
        kind.keyword(),
        CompletionItemKind::KEYWORD,
        Some("keyword"),
        Some(docs),
    );
    if let Some(snippet) = keyword_snippet(kind) {
        item.insert_text = Some(snippet);
        item.insert_text_format = Some(InsertTextFormat::SNIPPET);
    }
    item
}

fn keyword_snippet(kind: KeywordDoc) -> Option<String> {
    match kind {
        KeywordDoc::Record => Some("record { $0 };".to_string()),
        KeywordDoc::Variant => Some("variant { $0 };".to_string()),
        KeywordDoc::Import => Some("import \"${1:path}.did\";$0".to_string()),
        _ => None,
    }
}

fn semantic_completion_items(semantic: &Semantic, rope: &Rope) -> Vec<CompletionItem> {
    let import_symbol_ids: HashSet<SymbolId, RandomState> = semantic
        .table
        .imports
        .iter()
        .map(|entry| entry.symbol_id)
        .collect();

    let mut items = Vec::new();
    items.extend(symbol_completion_items(semantic, rope, &import_symbol_ids));
    items.extend(import_completion_items(semantic, rope));
    items
}

fn value_completion_items(
    semantic: &Semantic,
    rope: &Rope,
    offset: usize,
    snippet_style: ServiceSnippetStyle,
    scope_index: Option<&ScopeBindingIndex>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    items.extend(scoped_local_items(scope_index, offset));
    items.extend(service_method_snippets(
        semantic,
        rope,
        offset,
        snippet_style,
        None,
    ));

    let mut field_groups: BTreeMap<String, FieldGroup> = BTreeMap::new();
    for field in semantic.fields.iter() {
        let Some(label) = field
            .label
            .as_ref()
            .map(|text| text.as_ref().to_string())
            .or_else(|| {
                field
                    .label_span
                    .as_ref()
                    .and_then(|span| identifier_text(rope, span))
            })
        else {
            continue;
        };
        let entry = field_groups.entry(label).or_default();
        entry.parents.insert(field.parent_name.clone());
        if entry.docs.is_none() && field.docs.is_some() {
            entry.docs = field.docs.clone();
        }
    }
    for (label, group) in field_groups.into_iter() {
        let detail = field_detail(&group.parents);
        let docs = docs_to_markdown(group.docs.as_ref());
        items.push(build_completion_item(
            &label,
            CompletionItemKind::VALUE,
            detail.as_deref(),
            docs.as_deref(),
        ));
    }

    items.extend(service_method_labels(semantic, rope));

    for param in semantic.params.iter() {
        if let Some(span) = &param.name_span
            && let Some(label) = identifier_text(rope, span)
        {
            items.push(build_completion_item(
                &label,
                CompletionItemKind::VARIABLE,
                Some(param_detail(param.role)),
                None,
            ));
        }
    }

    if let Some(actor) = &semantic.actor
        && let Some(span) = &actor.name_span
        && let Some(label) = identifier_text(rope, span)
    {
        items.push(build_completion_item(
            &label,
            CompletionItemKind::INTERFACE,
            Some("actor"),
            docs_to_markdown(actor.docs.as_ref()).as_deref(),
        ));
    }

    items
}

fn lightweight_value_completion_items(
    semantic: Option<&Semantic>,
    rope: &Rope,
    offset: usize,
    scope_index: Option<&ScopeBindingIndex>,
) -> Vec<CompletionItem> {
    let mut items = scoped_local_items(scope_index, offset);
    if let Some(semantic) = semantic {
        items.extend(service_method_labels(semantic, rope));
    }
    items
}

fn scoped_local_items(
    scope_index: Option<&ScopeBindingIndex>,
    offset: usize,
) -> Vec<CompletionItem> {
    let Some(index) = scope_index else {
        return Vec::new();
    };
    let Some(bindings) = index.bindings_at(offset) else {
        return Vec::new();
    };
    bindings
        .iter()
        .map(|binding| {
            build_completion_item(
                binding.name.as_ref(),
                CompletionItemKind::VARIABLE,
                Some("local binding"),
                None,
            )
        })
        .collect()
}

fn service_method_items(
    semantic: &Semantic,
    rope: &Rope,
    offset: usize,
    style: ServiceSnippetStyle,
    cursor_context: Option<&CursorContext>,
    lightweight: bool,
) -> Vec<CompletionItem> {
    let mut items = service_method_labels(semantic, rope);
    if !lightweight {
        items.extend(service_method_snippets(
            semantic,
            rope,
            offset,
            style,
            cursor_context,
        ));
    }
    items
}

fn service_method_labels(semantic: &Semantic, rope: &Rope) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for method in semantic.service_methods.iter() {
        if let Some(span) = &method.name_span
            && let Some(label) = identifier_text(rope, span)
        {
            let detail = detail_with_parent("service method", method.parent_name.as_ref());
            let docs = docs_to_markdown(method.docs.as_ref());
            items.push(build_completion_item(
                &label,
                CompletionItemKind::METHOD,
                detail.as_deref(),
                docs.as_deref(),
            ));
        }
    }
    items
}

fn service_method_snippets(
    semantic: &Semantic,
    rope: &Rope,
    offset: usize,
    style: ServiceSnippetStyle,
    cursor_context: Option<&CursorContext>,
) -> Vec<CompletionItem> {
    let inside_block = cursor_context
        .map(|ctx| ctx.inside_service_block())
        .unwrap_or_else(|| inside_service_block(rope, offset));
    let mut items = Vec::new();
    for method in semantic.service_methods.iter() {
        if !inside_block && !span_contains(&method.span, offset) {
            continue;
        }
        if let Some(name_span) = &method.name_span
            && let Some(label) = identifier_text(rope, name_span)
        {
            let snippet = service_snippet_for(&label, method.signature.as_ref(), style);
            let docs = markdown::snippet_with_docs_markdown(&snippet, method.docs.as_deref());

            let detail = method_signature_detail(&label, method.signature.as_ref());
            let mut item = build_completion_item(
                &label,
                CompletionItemKind::SNIPPET,
                Some(&detail),
                docs.as_deref(),
            );
            item.insert_text = Some(snippet);
            item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            items.push(item);
        }
    }
    items
}

#[cfg(feature = "bench")]
pub mod bench_support {
    use super::*;
    use crate::{
        candid_lang::{ParserResult, parse},
        lsp::semantic_analyze::analyze_program,
    };
    use anyhow::{Result, anyhow};

    pub struct CompletionBenchFixture {
        source: String,
        rope: Rope,
        ast: IDLMergedProg,
        semantic: Semantic,
        cache: Option<CompletionDocumentCache>,
    }

    impl CompletionBenchFixture {
        pub fn load(text: &str) -> Result<Self> {
            let source = text.to_string();
            let rope = Rope::from_str(text);
            let ParserResult { ast, .. } = parse(text);
            let ast = ast.ok_or_else(|| anyhow!("failed to parse fixture"))?;
            let semantic = analyze_program(&ast, &rope)?;
            let cache = CompletionDocumentCache::build(Some(&ast), Some(&semantic), None);
            Ok(Self {
                source,
                rope,
                ast,
                semantic,
                cache,
            })
        }

        pub fn offset_of(&self, needle: &str) -> Option<usize> {
            let byte = self.source.find(needle)?;
            Some(self.rope.byte_to_char(byte))
        }

        pub fn completion_items_at(&self, offset: usize, style: ServiceSnippetStyle) -> usize {
            let cursor_context = CursorContext::new(&self.rope, offset);
            let context = determine_context(
                Some(offset),
                self.cache.as_ref(),
                Some(&self.semantic),
                Some(&self.ast),
                &self.rope,
                Some(&cursor_context),
                None,
            );
            let scope_index = self
                .cache
                .as_ref()
                .and_then(|cache| cache.scope_index(Some(&self.semantic), None));
            build_completion_items_sync(BuildCompletionItemsParams::new(
                context,
                Some(&self.semantic),
                &self.rope,
                style,
                scope_index,
                Some(offset),
                cursor_context.inside_service_block(),
                Some(&cursor_context),
                false,
            ))
            .len()
        }

        pub fn cursor_context_snapshot(&self, offset: usize) -> CursorContextSnapshot {
            let context = CursorContext::new(&self.rope, offset);
            CursorContextSnapshot {
                trimmed_len: context.trimmed_line.len(),
                depth: context.cursor_state.depth,
                inside_service_block: context.inside_service_block,
                inside_record_variant_block: context.inside_record_variant_block,
                in_comment: context.cursor_state.in_comment(),
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct CursorContextSnapshot {
        pub trimmed_len: usize,
        pub depth: usize,
        pub inside_service_block: bool,
        pub inside_record_variant_block: bool,
        pub in_comment: bool,
    }
}

fn service_snippet_for(
    name: &str,
    signature: Option<&MethodSignature>,
    style: ServiceSnippetStyle,
) -> String {
    let mut counter = PlaceholderCounter::new();
    let args = argument_placeholders(signature, &mut counter);
    let call = format!("{name}({args})");
    let awaited_call = format!("await {call}");
    let result_placeholder = format_result_placeholder(signature, &mut counter);

    match style {
        ServiceSnippetStyle::Call => append_result(call, result_placeholder),
        ServiceSnippetStyle::Await => append_result(awaited_call, result_placeholder),
        ServiceSnippetStyle::Async => {
            let mut inner = call;
            if let Some(result) = result_placeholder {
                push_result(&mut inner, &result);
            }
            format!("async {{ {inner} }}$0")
        }
        ServiceSnippetStyle::AwaitLet => {
            if let Some(result) = result_placeholder {
                format!("let {result} = {awaited_call};\n$0")
            } else {
                format!("{awaited_call};\n$0")
            }
        }
    }
}

fn append_result(call: String, placeholder: Option<String>) -> String {
    let mut text = call;
    if let Some(result) = placeholder {
        push_result(&mut text, &result);
    }
    text.push_str("$0");
    text
}

fn push_result(text: &mut String, result: &str) {
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(result);
}

fn argument_placeholders(
    signature: Option<&MethodSignature>,
    counter: &mut PlaceholderCounter,
) -> String {
    match signature {
        Some(sig) if sig.args.is_empty() => String::new(),
        Some(sig) => sig
            .args
            .iter()
            .map(|arg| counter.take(arg.as_ref().to_string()))
            .collect::<Vec<_>>()
            .join(", "),
        None => counter.take("args".to_string()),
    }
}

fn format_result_placeholder(
    signature: Option<&MethodSignature>,
    counter: &mut PlaceholderCounter,
) -> Option<String> {
    match signature {
        Some(sig) => {
            if sig.rets.is_empty() {
                None
            } else {
                let label = if sig.rets.len() == 1 {
                    format!("result : {}", sig.rets[0])
                } else {
                    format!("results : {}", tuple_text(&sig.rets))
                };
                Some(counter.take(label))
            }
        }
        None => Some(counter.take("result".to_string())),
    }
}

fn method_signature_detail(name: &str, signature: Option<&MethodSignature>) -> String {
    match signature {
        Some(signature) => {
            let args = tuple_text(&signature.args);
            let rets = tuple_text(&signature.rets);
            let mut detail = format!("{name} : {args} -> {rets}");
            if !signature.modes.is_empty() {
                let suffix = signature
                    .modes
                    .iter()
                    .map(func_mode_keyword)
                    .collect::<Vec<_>>()
                    .join(" ");
                detail.push(' ');
                detail.push_str(&suffix);
            }
            detail
        }
        None => "service call snippet".to_string(),
    }
}

fn tuple_text(values: &[Arc<str>]) -> String {
    if values.is_empty() {
        "()".to_string()
    } else {
        format!(
            "({})",
            values
                .iter()
                .map(|value| value.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn func_mode_keyword(mode: &FuncMode) -> &'static str {
    match mode {
        FuncMode::Oneway => "oneway",
        FuncMode::Query => "query",
        FuncMode::CompositeQuery => "composite_query",
    }
}

struct PlaceholderCounter {
    next_index: usize,
}

impl PlaceholderCounter {
    fn new() -> Self {
        Self { next_index: 1 }
    }

    fn take(&mut self, label: String) -> String {
        let current = self.next_index;
        self.next_index += 1;
        format!("${{{}:{}}}", current, label)
    }
}

fn symbol_completion_items(
    semantic: &Semantic,
    rope: &Rope,
    import_symbol_ids: &HashSet<SymbolId, RandomState>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (symbol_id, span) in semantic.table.symbol_id_to_span.iter_enumerated() {
        if import_symbol_ids.contains(&symbol_id) {
            continue;
        }
        let label = semantic
            .symbol_ident_names
            .get(symbol_id)
            .and_then(|opt| opt.clone())
            .map(|name| name.as_ref().to_string())
            .or_else(|| {
                let identifier_span = semantic
                    .symbol_ident_spans
                    .get(symbol_id)
                    .and_then(|opt| opt.clone())
                    .unwrap_or_else(|| span.clone());
                identifier_text(rope, &identifier_span)
            });
        if let Some(label) = label {
            let doc = semantic
                .type_docs
                .get(symbol_id)
                .and_then(|entry| entry.as_ref());
            items.push(build_type_completion_item(label, doc));
        }
    }
    items
}

fn import_completion_items(semantic: &Semantic, rope: &Rope) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for entry in semantic.table.imports.iter() {
        if let Some(label) = identifier_text(rope, &entry.span) {
            items.push(build_import_completion_item(label, entry));
        }
    }
    items
}

fn build_type_completion_item(label: String, doc: Option<&TypeDoc>) -> CompletionItem {
    let detail = doc.map(|doc| doc.definition.as_ref().to_string());
    let documentation = doc.map(type_doc_markdown);
    build_completion_item(
        &label,
        CompletionItemKind::STRUCT,
        detail.as_deref(),
        documentation.as_deref(),
    )
}

fn build_import_completion_item(label: String, entry: &ImportEntry) -> CompletionItem {
    let (kind, detail, noun) = match entry.kind {
        ImportKind::Type => (CompletionItemKind::STRUCT, "imported type", "type"),
        ImportKind::Service => (CompletionItemKind::INTERFACE, "imported service", "service"),
    };
    let docs = markdown::text_markdown(&format!("Imported {noun} from `{}`", entry.path));

    build_completion_item(&label, kind, Some(detail), docs.as_deref())
}

fn type_doc_markdown(doc: &TypeDoc) -> String {
    markdown::snippet_with_docs_markdown(doc.definition.as_ref(), doc.docs.as_deref())
        .unwrap_or_else(|| doc.definition.as_ref().to_string())
}

fn identifier_text(rope: &Rope, span: &Span) -> Option<String> {
    if span.start >= span.end || span.end > rope.len_chars() {
        return None;
    }

    let text = rope.slice(span.start..span.end).to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn push_if_unique(
    items: &mut Vec<CompletionItem>,
    seen: &mut HashSet<(String, Option<String>), RandomState>,
    item: CompletionItem,
) {
    let key = (item.label.clone(), item.detail.clone());
    if seen.insert(key) {
        items.push(item);
    }
}

fn append_sorted_items(
    items: &mut Vec<CompletionItem>,
    seen: &mut HashSet<(String, Option<String>), RandomState>,
    mut pending: Vec<CompletionItem>,
) {
    if pending.is_empty() {
        return;
    }
    pending.sort_by(|a, b| a.label.cmp(&b.label));
    for item in pending {
        push_if_unique(items, seen, item);
    }
}

fn spans_contain(spans: &[Span], offset: usize) -> bool {
    spans.iter().any(|span| span_contains(span, offset))
}

fn detail_with_parent(kind: &str, parent: Option<&Arc<str>>) -> Option<String> {
    parent
        .map(|parent| format!("{kind} of {parent}"))
        .or_else(|| Some(kind.to_string()))
}

fn field_detail(parents: &BTreeSet<Option<Arc<str>>>) -> Option<String> {
    let names: Vec<String> = parents
        .iter()
        .filter_map(|parent| parent.as_ref().map(|name| name.to_string()))
        .collect();
    if names.is_empty() {
        return Some("field".to_string());
    }
    const MAX_PARENTS: usize = 3;
    let mut detail = String::from("field of ");
    let display_count = names.len().min(MAX_PARENTS);
    detail.push_str(&names[..display_count].join(", "));
    if names.len() > MAX_PARENTS {
        detail.push_str(", ...");
    }
    Some(detail)
}

fn param_detail(role: ParamRole) -> &'static str {
    match role {
        ParamRole::Argument => "function argument",
        ParamRole::Result => "function result",
    }
}

fn docs_to_markdown(docs: Option<&Arc<str>>) -> Option<String> {
    docs.and_then(|doc| markdown::text_markdown(doc.as_ref()))
}

fn span_contains(span: &Span, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn span_len(span: &Span) -> usize {
    span.end.saturating_sub(span.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        candid_lang::{ParserResult, parse},
        lsp::{
            semantic_analyze::{
                FieldMetadata, MethodMetadata, ParamMetadata, Semantic, analyze_program,
            },
            symbol_table::{ImportEntry, ImportKind, SymbolTable},
            tasks::DocumentTaskState,
        },
    };
    use oxc_index::IndexVec;
    use ropey::Rope;
    use rust_lapper::Lapper;
    use tokio::task::yield_now;

    #[test]
    fn static_completion_contains_keyword_and_primitive() {
        let items = static_completion_items();
        assert!(items.iter().any(|item| item.label == "type"));
        assert!(items.iter().any(|item| item.label == "nat"));
        assert!(items.iter().any(|item| item.label == "blob"));
    }

    #[test]
    fn semantic_items_include_user_defined_types() {
        let text = "type Foo = record { value : nat32; };";
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");

        let items = semantic_completion_items(&semantic, &rope);
        assert!(
            items.iter().any(|item| item.label == "Foo"),
            "user-defined type not surfaced: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn semantic_items_include_imports() {
        let rope = Rope::from_str("Foo");
        let mut semantic = empty_semantic();
        let span = 0..3;
        let symbol_id = semantic.table.add_symbol(span.clone());
        semantic.symbol_ident_spans.push(None);
        semantic.type_docs.push(None);
        semantic.table.imports.push(ImportEntry {
            kind: ImportKind::Service,
            path: "foo.did".to_string(),
            span,
            symbol_id,
        });

        let items = semantic_completion_items(&semantic, &rope);
        let item = items
            .iter()
            .find(|item| item.label == "Foo")
            .expect("import completion");
        assert_eq!(item.detail.as_deref(), Some("imported service"));
        assert!(matches!(item.kind, Some(CompletionItemKind::INTERFACE)));
        let documentation = item.documentation.as_ref().expect("import doc");
        let Documentation::MarkupContent(markup) = documentation else {
            panic!("expected markup");
        };
        assert!(
            markup.value.contains("foo.did"),
            "missing import path: {}",
            markup.value
        );
    }

    #[test]
    fn type_context_detects_field_type_but_not_label() {
        let text = "type Foo = record { value : Bar; }; type Bar = null;";
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");

        let label_offset = text.find("value").expect("label");
        let spans = ContextSpans::from_sources(Some(&ast), Some(&semantic), Some(&rope))
            .expect("context spans");
        assert_eq!(spans.classify(label_offset), CompletionContext::Value);

        let type_offset = text.find("Bar").expect("type name");
        assert_eq!(spans.classify(type_offset), CompletionContext::Type);
    }

    #[test]
    fn value_completions_include_fields_and_methods() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");
        let scope_index = ScopeBindingIndex::from_semantic(&semantic);

        let offset = text.find("set_value").expect("set_value span");
        let items = value_completion_items(
            &semantic,
            &rope,
            offset,
            ServiceSnippetStyle::Call,
            scope_index.as_ref(),
        );
        assert!(
            items.iter().any(|item| item.label == "value"
                && item.detail.as_deref().unwrap_or("").contains("field")),
            "record field missing from value completions"
        );
        assert!(
            items.iter().any(|item| item.label == "get_value"),
            "service method missing from value completions"
        );
        assert!(
            items.iter().any(
                |item| item.label == "value" && item.detail.as_deref() == Some("local binding")
            ),
            "local binding missing from completions"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == "set_value"
                    && item.kind == Some(CompletionItemKind::SNIPPET)),
            "service snippets missing from completions"
        );
    }

    #[test]
    fn service_snippets_include_signature_details() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");

        let offset = text.find("get_value").expect("get_value span");
        let snippets =
            service_method_snippets(&semantic, &rope, offset, ServiceSnippetStyle::Await, None);
        let item = snippets
            .iter()
            .find(|item| item.label == "get_value")
            .expect("snippet item");
        assert_eq!(
            item.detail.as_deref(),
            Some("get_value : () -> (Foo) query"),
            "snippet detail should include signature"
        );
        assert_eq!(
            item.insert_text.as_deref(),
            Some("await get_value() ${1:result : Foo}$0"),
            "snippet should include result placeholder"
        );
    }

    #[test]
    fn service_snippets_include_argument_placeholders() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");

        let offset = text.find("set_value").expect("set_value span");
        let snippets =
            service_method_snippets(&semantic, &rope, offset, ServiceSnippetStyle::Call, None);
        let item = snippets
            .iter()
            .find(|item| item.label == "set_value")
            .expect("snippet item");
        let text = item.insert_text.as_deref().expect("insert text");
        assert!(
            text.contains("${1:") && text.contains("set_value(") && text.contains("Foo"),
            "argument placeholder missing type context: {text}"
        );
    }

    #[test]
    fn service_snippets_support_await_let_style() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");

        let offset = text.find("get_value").expect("get_value span");
        let snippets = service_method_snippets(
            &semantic,
            &rope,
            offset,
            ServiceSnippetStyle::AwaitLet,
            None,
        );
        let item = snippets
            .iter()
            .find(|item| item.label == "get_value")
            .expect("snippet item");
        assert_eq!(
            item.insert_text.as_deref(),
            Some("let ${1:result : Foo} = await get_value();\n$0"),
            "await-let snippet should bind awaited result"
        );
    }

    #[test]
    fn definition_context_detected_for_type_binding() {
        let text = "type Foo = record { value : nat32; };";
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");
        let spans = ContextSpans::from_sources(Some(&ast), Some(&semantic), Some(&rope))
            .expect("context spans");
        let offset = text.find("Foo").expect("binding identifier");
        assert_eq!(spans.classify(offset), CompletionContext::Definition);
    }

    #[test]
    fn heuristic_detects_type_context_after_equals() {
        let text = "type Hey = r";
        let rope = Rope::from_str(text);
        let offset = text.find('r').expect("char position");
        assert_eq!(
            heuristic_context(Some(offset), &rope),
            CompletionContext::Type
        );
    }

    #[test]
    fn heuristic_detects_definition_before_equals() {
        let text = "type He";
        let rope = Rope::from_str(text);
        let offset = text.len();
        assert_eq!(
            heuristic_context(Some(offset), &rope),
            CompletionContext::Definition
        );
    }

    #[test]
    fn heuristic_detects_value_context_inside_variant_field() {
        let text = "type Hey = variant {\n  n\n};";
        let rope = Rope::from_str(text);
        let start = text.find("{\n  n").expect("label chunk");
        let offset = start + "{\n  n".len();
        assert_eq!(
            heuristic_context(Some(offset), &rope),
            CompletionContext::Value
        );
    }

    #[test]
    fn heuristic_detects_value_context_inside_service_block() {
        let text = "service Hello : {\n  user_exists_by_id : () -> (bool) query;\n  u\n}";
        let rope = Rope::from_str(text);
        let needle = "  u";
        let byte_index = text.find(needle).expect("service block placeholder");
        let offset = rope.byte_to_char(byte_index + needle.len());
        assert_eq!(
            heuristic_context(Some(offset), &rope),
            CompletionContext::Value,
            "heuristic context near service method placeholder"
        );
    }

    #[test]
    fn symbol_completion_items_preserve_full_identifiers() {
        let text = r#"
            type PaperCategory = record { value : text };
            type PaperId = record { value : text };
            type Hao = record {
              name : PaperCategory;
              id : PaperId;
            };
        "#;
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic analysis");
        let items = semantic_completion_items(&semantic, &rope);
        let find_label = |needle: &str| items.iter().find(|item| item.label == needle);
        assert!(
            find_label("PaperCategory").is_some(),
            "PaperCategory missing"
        );
        assert!(find_label("PaperId").is_some(), "PaperId missing");
    }

    #[test]
    fn lightweight_value_completions_skip_snippets() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");
        let scope_index = ScopeBindingIndex::from_semantic(&semantic);

        let offset = text.find("set_value").expect("set_value span");
        let items = build_completion_items_sync(BuildCompletionItemsParams::new(
            CompletionContext::Value,
            Some(&semantic),
            &rope,
            ServiceSnippetStyle::Call,
            scope_index.as_ref(),
            Some(offset),
            false,
            None,
            true,
        ));

        assert!(
            items.iter().any(
                |item| item.label == "value" && item.kind == Some(CompletionItemKind::VARIABLE)
            ),
            "locals should remain available in lightweight mode"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == "get_value"
                    && item.kind == Some(CompletionItemKind::METHOD)),
            "service labels should be preserved"
        );
        assert!(
            !items
                .iter()
                .any(|item| item.kind == Some(CompletionItemKind::SNIPPET)),
            "lightweight mode should omit service snippets"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_builder_cancels_when_generation_advances() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/hover_sample.did"
        ));
        let rope = Rope::from_str(text);
        let ParserResult { ast, .. } = parse(text);
        let ast = ast.expect("parsed AST");
        let semantic = analyze_program(&ast, &rope).expect("semantic");
        let scope_index = ScopeBindingIndex::from_semantic(&semantic);
        let offset = text.find("set_value").expect("set_value span");
        let cursor_context = CursorContext::new(&rope, offset);

        let task_state = Arc::new(DocumentTaskState::default());
        let token = task_state.token(DocumentTaskKind::Completion);

        let build_future = build_completion_items_async(
            BuildCompletionItemsParams::new(
                CompletionContext::Value,
                Some(&semantic),
                &rope,
                ServiceSnippetStyle::Call,
                scope_index.as_ref(),
                Some(offset),
                cursor_context.inside_service_block(),
                Some(&cursor_context),
                false,
            ),
            token,
        );

        let cancel_handle = {
            let state = Arc::clone(&task_state);
            tokio::spawn(async move {
                yield_now().await;
                state.token(DocumentTaskKind::Completion);
            })
        };

        let outcome = build_future.await;
        cancel_handle.await.unwrap();

        match outcome {
            CompletionBuildOutcome::Cancelled => {}
            other => panic!("expected cancellation, got {:?}", other),
        }
    }

    #[test]
    fn record_keyword_completion_uses_snippet() {
        let items = keyword_completion_items();
        let record = items
            .iter()
            .find(|item| item.label == "record")
            .expect("record keyword");
        assert_eq!(
            record.insert_text.as_deref(),
            Some("record { $0 };"),
            "record keyword should insert braces snippet"
        );
        assert_eq!(
            record.insert_text_format,
            Some(InsertTextFormat::SNIPPET),
            "record keyword should use snippet format"
        );
    }

    fn empty_semantic() -> Semantic {
        Semantic {
            table: SymbolTable::default(),
            ident_range: Lapper::new(vec![]),
            fields: IndexVec::<_, FieldMetadata>::new(),
            service_methods: IndexVec::<_, MethodMetadata>::new(),
            params: IndexVec::<_, ParamMetadata>::new(),
            locals: Vec::new(),
            symbol_ident_spans: IndexVec::new(),
            symbol_ident_names: IndexVec::new(),
            type_docs: IndexVec::new(),
            primitive_spans: Vec::new(),
            keyword_spans: Vec::new(),
            actor: None,
        }
    }
}
