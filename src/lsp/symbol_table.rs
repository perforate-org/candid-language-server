use crate::lsp::span::Span;
use oxc_index::IndexVec;
use rapidhash::fast::RandomState;
use std::collections::HashMap;

oxc_index::define_index_type! {
    pub struct SymbolId = u32;
    IMPL_RAW_CONVERSIONS = true;
}

oxc_index::define_index_type! {
    pub struct ReferenceId = u32;
    IMPL_RAW_CONVERSIONS = true;
}

/// Lookup from symbol identifiers to the span where they were declared.
pub type SymbolIdToSpan = IndexVec<SymbolId, Span>;

/// Lookup from reference identifiers to their metadata.
pub type ReferenceIdToReference = IndexVec<ReferenceId, Reference>;

/// Bidirectional mapping between syntax spans, symbols, and their usages.
#[derive(Default, Debug)]
pub struct SymbolTable {
    pub span_to_symbol_id: HashMap<Span, SymbolId, RandomState>,
    pub symbol_id_to_span: SymbolIdToSpan,
    pub reference_id_to_reference: ReferenceIdToReference,
    pub span_to_reference_id: HashMap<Span, ReferenceId, RandomState>,
    pub symbol_id_to_references: HashMap<SymbolId, Vec<ReferenceId>, RandomState>,
    /// Records imported paths with their associated symbol identifiers.
    pub imports: Vec<ImportEntry>,
}

/// Describes a single usage of a symbol in the document.
#[derive(Debug)]
pub struct Reference {
    pub span: Span,
    pub symbol_id: Option<SymbolId>,
}

/// Categorises the kind of item brought in via an import declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Type,
    Service,
}

/// Captures metadata about an individual import directive.
#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub kind: ImportKind,
    pub path: String,
    pub span: Span,
    pub symbol_id: SymbolId,
}

impl SymbolTable {
    /// Register a symbol definition and return its identifier.
    pub fn add_symbol(&mut self, span: Span) -> SymbolId {
        let symbol_id = self.symbol_id_to_span.push(span.clone());
        self.span_to_symbol_id.insert(span.clone(), symbol_id);
        symbol_id
    }

    /// Record a reference occurrence and associate it with an optional symbol.
    pub fn add_reference(&mut self, span: Span, symbol_id: Option<SymbolId>) {
        let reference_id = self.reference_id_to_reference.push(Reference {
            span: span.clone(),
            symbol_id,
        });
        self.span_to_reference_id.insert(span, reference_id);
        if let Some(symbol_id) = symbol_id {
            self.symbol_id_to_references
                .entry(symbol_id)
                .or_default()
                .push(reference_id);
        }
    }

    /// Register an import statement, tracking both the path and its display span.
    pub fn add_import(&mut self, span: Span, path: String, kind: ImportKind) -> SymbolId {
        let symbol_id = self.add_symbol(span.clone());
        self.imports.push(ImportEntry {
            kind,
            path,
            span,
            symbol_id,
        });
        symbol_id
    }
}
