use crate::lsp::{
    CandidLanguageServer, lookup_identifier,
    navigation::IdentifierInfo,
    position::position_to_offset,
    semantic_analyze::{PrimitiveHover, Semantic},
    span::Span,
    span_to_range,
    symbol_table::ImportKind,
    type_docs::{TypeDoc, blob_doc, keyword_doc, primitive_doc},
};
use ropey::Rope;
use tower_lsp_server::{jsonrpc::Result, ls_types::*};

pub fn hover(server: &CandidLanguageServer, params: HoverParams) -> Result<Option<Hover>> {
    let response = (|| {
        let uri = params.text_document_position_params.text_document.uri;
        let uri_key = uri.to_string();
        let semantic = server.semantic_map.get(&uri_key)?;
        let rope = server.document_map.get(&uri_key)?;
        let position = params.text_document_position_params.position;
        let offset = position_to_offset(position, &rope)?;

        let info = lookup_identifier(&semantic, offset)?;
        let hover_range = span_to_range(&info.ident_span, &rope)?;
        let contents = hover_contents(&rope, &semantic, &info)?;

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
/// 1. `HoverContext` snapshots the current Rope/Semantic state.
/// 2. `HoverBuilder` classifies the identifier into a `HoverSubject`.
/// 3. The builder converts that subject plus any reference metadata into
///    ordered Markdown sections (`HoverSection`) before returning a LSP hover.
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

    fn snippet_from(&self, span: &Span) -> Option<String> {
        snippet_from_span(self.rope, span)
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
    sections: Vec<HoverSection>,
}

impl<'a> HoverBuilder<'a> {
    fn new(context: HoverContext<'a>) -> Self {
        Self {
            context,
            sections: Vec::new(),
        }
    }

    fn render(mut self) -> Option<HoverContents> {
        self.collect_subject_sections();
        self.collect_reference_sections();
        self.collect_definition_fallback();

        if self.sections.is_empty() {
            return None;
        }

        let value = self
            .sections
            .into_iter()
            .map(|section| section.render())
            .collect::<Vec<_>>()
            .join("\n\n");

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
                self.sections.push(HoverSection::code_block(snippet));
            }
            HoverSubject::Method {
                parent,
                snippet,
                docs,
            } => {
                if let Some(parent) = parent {
                    self.sections.push(HoverSection::code_block(parent));
                }
                push_snippet_with_docs(&mut self.sections, snippet, docs);
            }
            HoverSubject::Actor { snippet, docs } => {
                push_snippet_with_docs(&mut self.sections, snippet, docs);
            }
            HoverSubject::Field {
                parent,
                snippet,
                docs,
            } => {
                if let Some(parent) = parent {
                    self.sections.push(HoverSection::code_block(parent));
                }
                push_snippet_with_docs(&mut self.sections, snippet, docs);
            }
            HoverSubject::TypeDoc { definition, docs } => {
                push_snippet_with_docs(&mut self.sections, definition, docs);
            }
            HoverSubject::Ident { snippet } => {
                self.sections.push(HoverSection::code_block(snippet));
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
            self.sections.push(HoverSection::text(doc));
        }

        if let Some(keyword) = info.keyword.and_then(keyword_doc) {
            self.sections.push(HoverSection::text(keyword));
        }

        if let Some(import) = &info.import {
            let kind = match import.kind {
                ImportKind::Type => "type",
                ImportKind::Service => "service",
            };
            self.sections.push(HoverSection::text(format!(
                "Imported {kind} from `{}`",
                import.path
            )));
        }
    }

    fn collect_definition_fallback(&mut self) {
        let info = self.context.info();
        if let Some(def_span) = &info.definition_span
            && self.sections.is_empty()
            && def_span != &info.ident_span
            && let Some(def_snippet) = self.context.snippet_from(def_span)
        {
            self.sections
                .push(HoverSection::text(format!("Definition: `{def_snippet}`")));
        }
    }

    fn extract_subject(&self) -> HoverSubject {
        let info = self.context.info();

        if let Some(param_info) = &info.param
            && let Some(param_snippet) = self.context.snippet_from(&param_info.span)
        {
            return HoverSubject::Param {
                snippet: param_snippet,
            };
        }

        if let Some(method_info) = &info.service_method
            && let Some(method_snippet) = self.context.snippet_from(&method_info.span)
        {
            return HoverSubject::Method {
                parent: method_info.parent_name.clone(),
                snippet: method_snippet,
                docs: method_info.docs.clone(),
            };
        }

        if let Some(actor_info) = &info.actor {
            if let Some(definition) = &actor_info.definition {
                return HoverSubject::Actor {
                    snippet: definition.clone(),
                    docs: actor_info.docs.clone(),
                };
            } else if let Some(actor_snippet) = self.context.snippet_from(&actor_info.span) {
                return HoverSubject::Actor {
                    snippet: actor_snippet,
                    docs: actor_info.docs.clone(),
                };
            }
        }

        if let Some(field_info) = &info.field
            && let Some(field_snippet) = self.context.snippet_from(&field_info.span)
        {
            return HoverSubject::Field {
                parent: field_info.parent_name.clone(),
                snippet: field_snippet,
                docs: field_info.docs.clone(),
            };
        }

        if let Some(type_doc) = self.context.type_doc() {
            return HoverSubject::TypeDoc {
                definition: type_doc.definition.clone(),
                docs: type_doc.docs.clone(),
            };
        }

        if let Some(ident_snippet) = self.context.snippet_from(&info.ident_span) {
            return HoverSubject::Ident {
                snippet: ident_snippet,
            };
        }

        HoverSubject::None
    }
}

enum HoverSubject {
    Param {
        snippet: String,
    },
    Method {
        parent: Option<String>,
        snippet: String,
        docs: Option<String>,
    },
    Actor {
        snippet: String,
        docs: Option<String>,
    },
    Field {
        parent: Option<String>,
        snippet: String,
        docs: Option<String>,
    },
    TypeDoc {
        definition: String,
        docs: Option<String>,
    },
    Ident {
        snippet: String,
    },
    None,
}

#[derive(Clone)]
enum HoverSection {
    CodeBlock {
        language: &'static str,
        code: String,
    },
    Text(String),
    Rule,
}

impl HoverSection {
    fn code_block(code: String) -> Self {
        HoverSection::CodeBlock {
            language: "candid",
            code,
        }
    }

    fn text<T: Into<String>>(value: T) -> Self {
        HoverSection::Text(value.into())
    }

    fn render(self) -> String {
        match self {
            HoverSection::CodeBlock { language, code } => {
                format!("```{language}\n{code}\n```")
            }
            HoverSection::Text(text) => text,
            HoverSection::Rule => "---".to_string(),
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

fn push_snippet_with_docs(sections: &mut Vec<HoverSection>, snippet: String, docs: Option<String>) {
    sections.push(HoverSection::code_block(snippet));
    push_docs_section(sections, docs);
}

fn push_docs_section(sections: &mut Vec<HoverSection>, docs: Option<String>) {
    if let Some(doc) = docs {
        sections.push(HoverSection::Rule);
        sections.push(HoverSection::text(doc));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{
        semantic_analyze::{
            FieldMetadata, MethodMetadata, ParamMetadata, PrimitiveHover, Semantic,
        },
        symbol_table::{SymbolId, SymbolTable},
    };
    use candid_parser::syntax::PrimType;
    use oxc_index::IndexVec;
    use ropey::Rope;
    use rust_lapper::Lapper;

    fn base_semantic() -> Semantic {
        Semantic {
            table: SymbolTable::default(),
            ident_range: Lapper::new(vec![]),
            fields: IndexVec::<_, FieldMetadata>::new(),
            service_methods: IndexVec::<_, MethodMetadata>::new(),
            params: IndexVec::<_, ParamMetadata>::new(),
            symbol_ident_spans: IndexVec::new(),
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
            definition: "type Foo = nat".to_string(),
            docs: None,
        }));
        let rope = Rope::from_str("type Foo = nat");
        let info = IdentifierInfo {
            ident_span: 5..8,
            definition_span: Some(5..8),
            symbol_id: Some(SymbolId::from_raw(0)),
            reference_id: None,
            import: None,
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
            import: None,
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
}
