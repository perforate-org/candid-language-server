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

pub fn hover_contents(
    rope: &Rope,
    semantic: &Semantic,
    info: &IdentifierInfo,
) -> Option<HoverContents> {
    let mut sections = Vec::new();
    let has_inline_doc = info.primitive.is_some() || info.keyword.is_some();

    if !has_inline_doc {
        if let Some(param_info) = &info.param
            && let Some(param_snippet) = snippet_from_span(rope, &param_info.span)
        {
            sections.push(format!("```candid\n{param_snippet}\n```"));
        } else if let Some(method_info) = &info.service_method
            && let Some(method_snippet) = snippet_from_span(rope, &method_info.span)
        {
            if let Some(parent) = &method_info.parent_name {
                sections.push(format!("```candid\n{parent}\n```"));
            }
            push_snippet_with_docs(&mut sections, method_snippet, &method_info.docs);
        } else if let Some(actor_info) = &info.actor {
            if let Some(definition) = &actor_info.definition {
                push_snippet_with_docs(&mut sections, definition.clone(), &actor_info.docs);
            } else if let Some(actor_snippet) = snippet_from_span(rope, &actor_info.span) {
                push_snippet_with_docs(&mut sections, actor_snippet, &actor_info.docs);
            }
        } else if let Some(field_info) = &info.field
            && let Some(field_snippet) = snippet_from_span(rope, &field_info.span)
        {
            if let Some(parent) = &field_info.parent_name {
                sections.push(format!("```candid\n{parent}\n```"));
            }
            push_snippet_with_docs(&mut sections, field_snippet, &field_info.docs);
        } else if let Some(type_doc) = type_section(semantic, info) {
            push_snippet_with_docs(&mut sections, type_doc.definition, &type_doc.docs);
        } else if let Some(ident_snippet) = snippet_from_span(rope, &info.ident_span) {
            sections.push(format!("```candid\n{ident_snippet}\n```"));
        }
    }

    if let Some(prim) = &info.primitive {
        let doc = match prim {
            PrimitiveHover::Prim(kind) => primitive_doc(kind),
            PrimitiveHover::Blob => blob_doc(),
        };
        sections.push(doc);
    }

    if let Some(keyword) = info.keyword.and_then(keyword_doc) {
        sections.push(keyword);
    }

    if let Some(import) = &info.import {
        let kind = match import.kind {
            ImportKind::Type => "type",
            ImportKind::Service => "service",
        };
        sections.push(format!("Imported {kind} from `{}`", import.path));
    }

    if let Some(def_span) = &info.definition_span
        && sections.is_empty()
        && def_span != &info.ident_span
        && let Some(def_snippet) = snippet_from_span(rope, def_span)
    {
        sections.push(format!("Definition: `{def_snippet}`"));
    }

    if sections.is_empty() {
        return None;
    }

    let value = sections.join("\n\n");
    Some(HoverContents::Markup(MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }))
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

fn type_section(semantic: &Semantic, info: &IdentifierInfo) -> Option<TypeDoc> {
    let symbol_id = info.symbol_id?;
    semantic.type_docs.get(symbol_id)?.clone()
}

fn push_snippet_with_docs(sections: &mut Vec<String>, snippet: String, docs: &Option<String>) {
    sections.push(format!("```candid\n{snippet}\n```"));
    push_docs_section(sections, docs);
}

fn push_docs_section(sections: &mut Vec<String>, docs: &Option<String>) {
    if let Some(doc) = docs {
        sections.push("---".to_string());
        sections.push(doc.clone());
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
