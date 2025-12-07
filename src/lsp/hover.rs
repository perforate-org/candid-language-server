use crate::lsp::{
    CandidLanguageServer, lookup_identifier, navigation::IdentifierInfo,
    position::offset_to_position, position_to_offset, span::Span, span_to_range,
    symbol_table::ImportKind,
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
        let contents = hover_contents(&rope, &info)?;

        Some(Hover {
            contents,
            range: Some(hover_range),
        })
    })();

    Ok(response)
}

pub fn hover_contents(rope: &Rope, info: &IdentifierInfo) -> Option<HoverContents> {
    let mut sections = Vec::new();

    let ident_snippet = snippet_from_span(rope, &info.ident_span)?;
    sections.push(format!("```candid\n{ident_snippet}\n```"));

    if let Some(import) = &info.import {
        let kind = match import.kind {
            ImportKind::Type => "type",
            ImportKind::Service => "service",
        };
        sections.push(format!("Imported {kind} from `{}`", import.path));
    }

    if let Some(def_span) = &info.definition_span {
        if def_span != &info.ident_span
            && let Some(def_snippet) = snippet_from_span(rope, def_span)
        {
            sections.push(format!("Definition: `{def_snippet}`"));
        }

        if let Some(start_pos) = offset_to_position(def_span.start, rope) {
            sections.push(format!(
                "Defined at line {} column {}",
                start_pos.line + 1,
                start_pos.character + 1
            ));
        }
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
