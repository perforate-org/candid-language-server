use crate::lsp::{
    CandidLanguageServer, lookup_identifier,
    markdown::{self, MarkdownWriter},
    navigation::IdentifierInfo,
    semantic_analyze::{PrimitiveHover, Semantic},
    span::Span,
    span_to_range,
    symbol_table::ImportKind,
    type_docs::{TypeDoc, blob_doc, keyword_doc, primitive_doc},
};
use rapidhash::fast::RandomState;
use ropey::Rope;
use std::{collections::HashMap, sync::Arc};
use tower_lsp_server::{jsonrpc::Result, ls_types::*};

pub fn hover(server: &CandidLanguageServer, params: HoverParams) -> Result<Option<Hover>> {
    let response = (|| {
        let uri = params.text_document_position_params.text_document.uri;
        let uri_key = uri.to_string();
        let analysis = server.analysis_map.get(&uri_key)?;
        let document = server.documents.get(&uri_key)?;
        let semantic = analysis.semantic()?;
        let rope = document.rope();
        let version = document.version();
        let position = params.text_document_position_params.position;
        let offset = server.cached_position_to_offset(&uri_key, position, rope, version)?;

        let info = lookup_identifier(semantic, offset)?;
        let hover_range = span_to_range(&info.ident_span, rope)?;
        let contents = hover_contents(rope, semantic, &info)?;

        Some(Hover {
            contents,
            range: Some(hover_range),
        })
    })();

    Ok(response)
}

/// Compose hover markup for the identifier located at `info.ident_span`.
///
/// The rendering pipeline has three steps:
/// 1. [`HoverContext`] snapshots the current Rope/Semantic state.
/// 2. [`HoverBuilder`] classifies the identifier into a `HoverSubject`.
/// 3. The builder converts that subject plus any reference metadata into
///    ordered Markdown sections via [`MarkdownWriter`] before returning a LSP hover.
pub fn hover_contents(
    rope: &Rope,
    semantic: &Semantic,
    info: &IdentifierInfo,
) -> Option<HoverContents> {
    let context = HoverContext::new(rope, semantic, info);
    HoverBuilder::new(context).render()
}

struct HoverContext<'a> {
    rope: &'a Rope,
    semantic: &'a Semantic,
    info: &'a IdentifierInfo,
}

impl<'a> HoverContext<'a> {
    fn new(rope: &'a Rope, semantic: &'a Semantic, info: &'a IdentifierInfo) -> Self {
        Self {
            rope,
            semantic,
            info,
        }
    }

    fn info(&self) -> &IdentifierInfo {
        self.info
    }

    fn semantic(&self) -> &Semantic {
        self.semantic
    }

    fn rope(&self) -> &Rope {
        self.rope
    }

    fn has_inline_doc(&self) -> bool {
        self.info.primitive.is_some() || self.info.keyword.is_some()
    }

    fn type_doc(&self) -> Option<&'a TypeDoc> {
        let symbol_id = self.info.symbol_id?;
        self.semantic.type_docs.get(symbol_id)?.as_ref()
    }
}

/// Incrementally assembles Markdown sections for a hover response.
struct HoverBuilder<'a> {
    context: HoverContext<'a>,
    writer: MarkdownWriter,
    snippet_cache: SnippetCache,
}

impl<'a> HoverBuilder<'a> {
    fn new(context: HoverContext<'a>) -> Self {
        Self {
            context,
            writer: MarkdownWriter::default(),
            snippet_cache: SnippetCache::new(),
        }
    }

    fn render(mut self) -> Option<HoverContents> {
        self.collect_subject_sections();
        self.collect_reference_sections();
        self.collect_definition_fallback();

        let value = self.writer.finish()?;

        Some(HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }))
    }

    fn collect_subject_sections(&mut self) {
        if self.context.has_inline_doc() {
            return;
        }

        let subject = self.extract_subject();
        match subject {
            HoverSubject::Param { snippet } => {
                self.writer.push_code_block(&snippet);
            }
            HoverSubject::Method {
                parent,
                snippet,
                docs,
            } => {
                if let Some(parent) = parent {
                    self.writer.push_code_block(&parent);
                }
                markdown::push_snippet_with_docs(&mut self.writer, &snippet, docs.as_deref());
            }
            HoverSubject::Actor { snippet, docs } => {
                markdown::push_snippet_with_docs(&mut self.writer, &snippet, docs.as_deref());
            }
            HoverSubject::Field {
                parent,
                snippet,
                docs,
            } => {
                if let Some(parent) = parent {
                    self.writer.push_code_block(&parent);
                }
                markdown::push_snippet_with_docs(&mut self.writer, &snippet, docs.as_deref());
            }
            HoverSubject::TypeDoc { definition, docs } => {
                markdown::push_snippet_with_docs(&mut self.writer, &definition, docs.as_deref());
            }
            HoverSubject::Ident { snippet } => {
                self.writer.push_code_block(&snippet);
            }
            HoverSubject::None => {}
        }
    }

    fn collect_reference_sections(&mut self) {
        let info = self.context.info();

        if let Some(prim) = &info.primitive {
            let doc = match prim {
                PrimitiveHover::Prim(kind) => primitive_doc(kind),
                PrimitiveHover::Blob => blob_doc(),
            };
            self.writer.push_text(doc);
        }

        if let Some(keyword) = info.keyword.and_then(keyword_doc) {
            self.writer.push_text(keyword);
        }

        if let Some(symbol_id) = info.symbol_id
            && let Some(import) = self
                .context
                .semantic()
                .table
                .imports
                .iter()
                .find(|entry| entry.symbol_id == symbol_id)
        {
            let kind = match import.kind {
                ImportKind::Type => "type",
                ImportKind::Service => "service",
            };
            self.writer
                .push_text(format!("Imported {kind} from `{}`", import.path));
        }
    }

    fn collect_definition_fallback(&mut self) {
        let info = self.context.info().clone();
        if let Some(def_span) = &info.definition_span
            && self.writer.is_empty()
            && def_span != &info.ident_span
            && let Some(def_snippet) = self.snippet(def_span)
        {
            self.writer
                .push_text(format!("Definition: `{}`", def_snippet.as_ref()));
        }
    }

    fn snippet(&mut self, span: &Span) -> Option<Arc<str>> {
        self.snippet_cache.get(self.context.rope(), span)
    }

    fn extract_subject(&mut self) -> HoverSubject {
        let info = self.context.info().clone();

        if let Some(param_ref) = info.param
            && let Some(span) = self
                .context
                .semantic()
                .params
                .get(param_ref.id)
                .map(|metadata| metadata.span.clone())
            && let Some(param_snippet) = self.snippet(&span)
        {
            return HoverSubject::Param {
                snippet: param_snippet,
            };
        }

        if let Some(method_ref) = info.service_method
            && let Some((span, parent, docs)) = self
                .context
                .semantic()
                .service_methods
                .get(method_ref.id)
                .map(|metadata| {
                    (
                        metadata.span.clone(),
                        metadata.parent_name.clone(),
                        metadata.docs.clone(),
                    )
                })
            && let Some(method_snippet) = self.snippet(&span)
        {
            return HoverSubject::Method {
                parent,
                snippet: method_snippet,
                docs,
            };
        }

        if info.actor.is_some()
            && let Some((definition, span, docs)) =
                self.context.semantic().actor.as_ref().map(|actor| {
                    (
                        actor.definition.clone(),
                        actor.span.clone(),
                        actor.docs.clone(),
                    )
                })
        {
            if let Some(definition) = definition {
                return HoverSubject::Actor {
                    snippet: definition,
                    docs,
                };
            }

            if let Some(actor_snippet) = self.snippet(&span) {
                return HoverSubject::Actor {
                    snippet: actor_snippet,
                    docs,
                };
            }
        }

        if let Some(field_ref) = info.field
            && let Some((span, parent, docs)) = self
                .context
                .semantic()
                .fields
                .get(field_ref.id)
                .map(|metadata| {
                    (
                        metadata.span.clone(),
                        metadata.parent_name.clone(),
                        metadata.docs.clone(),
                    )
                })
            && let Some(field_snippet) = self.snippet(&span)
        {
            return HoverSubject::Field {
                parent,
                snippet: field_snippet,
                docs,
            };
        }

        if let Some(type_doc) = self.context.type_doc() {
            return HoverSubject::TypeDoc {
                definition: type_doc.definition.clone(),
                docs: type_doc.docs.clone(),
            };
        }

        if let Some(ident_snippet) = self.snippet(&info.ident_span) {
            return HoverSubject::Ident {
                snippet: ident_snippet,
            };
        }

        HoverSubject::None
    }
}

enum HoverSubject {
    Param {
        snippet: Arc<str>,
    },
    Method {
        parent: Option<Arc<str>>,
        snippet: Arc<str>,
        docs: Option<Arc<str>>,
    },
    Actor {
        snippet: Arc<str>,
        docs: Option<Arc<str>>,
    },
    Field {
        parent: Option<Arc<str>>,
        snippet: Arc<str>,
        docs: Option<Arc<str>>,
    },
    TypeDoc {
        definition: Arc<str>,
        docs: Option<Arc<str>>,
    },
    Ident {
        snippet: Arc<str>,
    },
    None,
}

struct SnippetCache {
    map: HashMap<(usize, usize), Option<Arc<str>>, RandomState>,
}

impl SnippetCache {
    fn new() -> Self {
        Self {
            map: HashMap::with_hasher(RandomState::new()),
        }
    }

    fn get(&mut self, rope: &Rope, span: &Span) -> Option<Arc<str>> {
        let key = (span.start, span.end);
        match self.map.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.get().clone(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let computed = snippet_from_span(rope, span)
                    .map(|snippet| Arc::<str>::from(snippet.into_boxed_str()));
                let value = computed.clone();
                entry.insert(computed);
                value
            }
        }
    }
}

pub fn snippet_from_span(rope: &Rope, span: &Span) -> Option<String> {
    if span.start >= span.end {
        return None;
    }

    let text = rope.slice(span.start..span.end).to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut lines = trimmed.lines();
    let first_line = lines.next()?;
    let snippet = if lines.next().is_some() {
        format!("{} …", first_line.trim_end())
    } else {
        first_line.to_string()
    };

    Some(snippet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{
        semantic_analyze::{
            FieldMetadata, MethodMetadata, ParamMetadata, PrimitiveHover, Semantic,
        },
        symbol_table::{ImportEntry, ImportKind, SymbolId, SymbolTable},
    };
    use candid_parser::syntax::PrimType;
    use oxc_index::IndexVec;
    use ropey::Rope;
    use rust_lapper::Lapper;
    use std::sync::Arc;

    fn base_semantic() -> Semantic {
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

    #[test]
    fn hover_includes_type_doc() {
        let mut semantic = base_semantic();
        semantic.symbol_ident_spans.push(None);
        semantic.type_docs.push(Some(TypeDoc {
            definition: Arc::<str>::from("type Foo = nat"),
            docs: None,
        }));
        let rope = Rope::from_str("type Foo = nat");
        let info = IdentifierInfo {
            ident_span: 5..8,
            definition_span: Some(5..8),
            symbol_id: Some(SymbolId::from_raw(0)),
            reference_id: None,
            field: None,
            service_method: None,
            param: None,
            primitive: None,
            keyword: None,
            actor: None,
        };

        let hover = hover_contents(&rope, &semantic, &info).expect("hover");
        let HoverContents::Markup(content) = hover else {
            panic!("expected markup");
        };
        assert!(
            content.value.contains("type Foo = nat"),
            "missing rendered type doc"
        );
    }

    #[test]
    fn hover_includes_primitive_doc() {
        let semantic = base_semantic();
        let rope = Rope::from_str("let x : nat = 0;");
        let info = IdentifierInfo {
            ident_span: 9..12,
            definition_span: Some(9..12),
            symbol_id: None,
            reference_id: None,
            field: None,
            service_method: None,
            param: None,
            primitive: Some(PrimitiveHover::Prim(PrimType::Nat)),
            keyword: None,
            actor: None,
        };

        let hover = hover_contents(&rope, &semantic, &info).expect("hover");
        let HoverContents::Markup(content) = hover else {
            panic!("expected markup");
        };
        assert!(
            content.value.contains("```candid\nnat\n```"),
            "primitive header missing"
        );
        assert!(
            content.value.contains("Unbounded non-negative"),
            "primitive description missing"
        );
    }

    #[test]
    fn hover_definition_fallback_triggers_when_no_sections() {
        let semantic = base_semantic();
        let rope = Rope::from_str("type Foo = nat");
        let info = IdentifierInfo {
            ident_span: 0..0,
            definition_span: Some(5..8),
            symbol_id: None,
            reference_id: None,
            field: None,
            service_method: None,
            param: None,
            primitive: None,
            keyword: None,
            actor: None,
        };

        let hover = hover_contents(&rope, &semantic, &info).expect("hover");
        let HoverContents::Markup(content) = hover else {
            panic!("expected markup");
        };
        assert!(
            content.value.contains("Definition: `Foo`"),
            "fallback definition missing: {}",
            content.value
        );
    }

    #[test]
    fn hover_includes_import_metadata() {
        let mut semantic = base_semantic();
        semantic.table.imports.push(ImportEntry {
            kind: ImportKind::Service,
            path: "foo.did".to_string(),
            span: 0..3,
            symbol_id: SymbolId::from_raw(0),
        });
        let rope = Rope::from_str("Foo");
        let info = IdentifierInfo {
            ident_span: 0..3,
            definition_span: None,
            symbol_id: Some(SymbolId::from_raw(0)),
            reference_id: None,
            field: None,
            service_method: None,
            param: None,
            primitive: None,
            keyword: None,
            actor: None,
        };

        let hover = hover_contents(&rope, &semantic, &info).expect("hover");
        let HoverContents::Markup(content) = hover else {
            panic!("expected markup");
        };
        assert!(
            content.value.contains("Imported service from `foo.did`"),
            "import metadata missing: {}",
            content.value
        );
    }
}
