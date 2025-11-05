use crate::lsp::{
    semantic_analyze::{IdentType, Semantic},
    span::Span,
    symbol_table::{ImportEntry, ReferenceId, SymbolId},
};

/// Captures information about the identifier located at a given position.
#[derive(Debug, Clone)]
pub struct IdentifierInfo {
    pub ident_span: Span,
    pub definition_span: Option<Span>,
    pub symbol_id: Option<SymbolId>,
    pub reference_id: Option<ReferenceId>,
    pub import: Option<ImportEntry>,
}

/// Locate the identifier overlapping the provided offset.
pub fn lookup_identifier(semantic: &Semantic, offset: usize) -> Option<IdentifierInfo> {
    let interval = semantic.ident_range.find(offset, offset + 1).next()?;

    match interval.val {
        IdentType::Binding(symbol_id) => {
            let span = semantic.table.symbol_id_to_span.get(symbol_id)?.clone();
            let import = semantic
                .table
                .imports
                .iter()
                .find(|entry| entry.symbol_id == symbol_id)
                .cloned();

            Some(IdentifierInfo {
                ident_span: span.clone(),
                definition_span: Some(span),
                symbol_id: Some(symbol_id),
                reference_id: None,
                import,
            })
        }
        IdentType::Reference(reference_id) => {
            let reference = semantic.table.reference_id_to_reference.get(reference_id)?;
            let symbol_id = reference.symbol_id;
            let definition_span =
                symbol_id.and_then(|sid| semantic.table.symbol_id_to_span.get(sid).cloned());
            let import = symbol_id.and_then(|sid| {
                semantic
                    .table
                    .imports
                    .iter()
                    .find(|entry| entry.symbol_id == sid)
                    .cloned()
            });

            Some(IdentifierInfo {
                ident_span: reference.span.clone(),
                definition_span,
                symbol_id,
                reference_id: Some(reference_id),
                import,
            })
        }
    }
}
