use crate::lsp::{
    semantic_analyze::{FieldPart, IdentType, ParamRole, PrimitiveHover, Semantic},
    span::Span,
    symbol_table::{ImportEntry, ReferenceId, SymbolId},
    type_docs::KeywordDoc,
};

/// Captures information about the identifier located at a given position.
#[derive(Debug, Clone)]
pub struct IdentifierInfo {
    pub ident_span: Span,
    pub definition_span: Option<Span>,
    pub symbol_id: Option<SymbolId>,
    pub reference_id: Option<ReferenceId>,
    pub import: Option<ImportEntry>,
    pub field: Option<FieldIdentifier>,
    pub service_method: Option<MethodIdentifier>,
    pub param: Option<ParamIdentifier>,
    pub primitive: Option<PrimitiveHover>,
    pub keyword: Option<KeywordDoc>,
    pub actor: Option<ActorIdentifier>,
}

#[derive(Debug, Clone)]
pub struct FieldIdentifier {
    pub span: Span,
    pub label_span: Option<Span>,
    pub type_span: Option<Span>,
    pub role: FieldRole,
    pub docs: Option<String>,
    pub parent_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MethodIdentifier {
    pub span: Span,
    pub type_span: Option<Span>,
    pub docs: Option<String>,
    pub parent_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParamIdentifier {
    pub span: Span,
    pub type_span: Span,
    pub role: ParamRole,
}

#[derive(Debug, Clone)]
pub struct ActorIdentifier {
    pub span: Span,
    pub docs: Option<String>,
    pub definition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRole {
    Label,
    Type,
}

/// Locate the identifier overlapping the provided offset.
pub fn lookup_identifier(semantic: &Semantic, offset: usize) -> Option<IdentifierInfo> {
    let mut best: Option<(u8, usize, IdentType, Span)> = None;
    for interval in semantic.ident_range.find(offset, offset + 1) {
        let priority = ident_priority(&interval.val);
        let span_len = interval.stop.saturating_sub(interval.start);
        let replace = match &best {
            Some((best_priority, best_len, _, _)) => {
                priority > *best_priority || (priority == *best_priority && span_len < *best_len)
            }
            None => true,
        };

        if replace {
            best = Some((
                priority,
                span_len,
                interval.val.clone(),
                interval.start..interval.stop,
            ));
        }
    }

    let (_, _, ident_type, span) = best?;

    match ident_type {
        IdentType::Binding(symbol_id) => {
            let fallback_span = semantic.table.symbol_id_to_span.get(symbol_id)?.clone();
            let span = semantic
                .symbol_ident_spans
                .get(symbol_id)
                .and_then(|opt| opt.clone())
                .unwrap_or(fallback_span.clone());
            let import = semantic
                .table
                .imports
                .iter()
                .find(|entry| entry.symbol_id == symbol_id)
                .cloned();

            Some(IdentifierInfo {
                ident_span: span,
                definition_span: Some(fallback_span),
                symbol_id: Some(symbol_id),
                reference_id: None,
                import,
                field: None,
                service_method: None,
                param: None,
                primitive: None,
                keyword: None,
                actor: None,
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
                field: None,
                service_method: None,
                param: None,
                primitive: None,
                keyword: None,
                actor: None,
            })
        }
        IdentType::Field(field_id, part) => {
            let metadata = semantic.fields.get(field_id)?;
            let ident_span = match part {
                FieldPart::Label => metadata.label_span.clone()?,
                FieldPart::Type => metadata.type_span.clone()?,
            };
            Some(IdentifierInfo {
                ident_span,
                definition_span: Some(metadata.span.clone()),
                symbol_id: None,
                reference_id: None,
                import: None,
                field: Some(FieldIdentifier {
                    span: metadata.span.clone(),
                    label_span: metadata.label_span.clone(),
                    type_span: metadata.type_span.clone(),
                    role: match part {
                        FieldPart::Label => FieldRole::Label,
                        FieldPart::Type => FieldRole::Type,
                    },
                    docs: metadata.docs.clone(),
                    parent_name: metadata.parent_name.clone(),
                }),
                service_method: None,
                param: None,
                primitive: None,
                keyword: None,
                actor: None,
            })
        }
        IdentType::ServiceMethod(method_id) => {
            let metadata = semantic.service_methods.get(method_id)?;
            let ident_span = metadata.name_span.clone()?;
            Some(IdentifierInfo {
                ident_span,
                definition_span: Some(metadata.span.clone()),
                symbol_id: None,
                reference_id: None,
                import: None,
                field: None,
                service_method: Some(MethodIdentifier {
                    span: metadata.span.clone(),
                    type_span: metadata.type_span.clone(),
                    docs: metadata.docs.clone(),
                    parent_name: metadata.parent_name.clone(),
                }),
                param: None,
                primitive: None,
                keyword: None,
                actor: None,
            })
        }
        IdentType::FuncParam(param_id) => {
            let metadata = semantic.params.get(param_id)?;
            let ident_span = metadata.name_span.clone()?;
            Some(IdentifierInfo {
                ident_span,
                definition_span: Some(metadata.span.clone()),
                symbol_id: None,
                reference_id: None,
                import: None,
                field: None,
                service_method: None,
                param: Some(ParamIdentifier {
                    span: metadata.span.clone(),
                    type_span: metadata.type_span.clone(),
                    role: metadata.role,
                }),
                primitive: None,
                keyword: None,
                actor: None,
            })
        }
        IdentType::Primitive(kind) => Some(IdentifierInfo {
            ident_span: span.clone(),
            definition_span: None,
            symbol_id: None,
            reference_id: None,
            import: None,
            field: None,
            service_method: None,
            param: None,
            primitive: Some(kind),
            keyword: None,
            actor: None,
        }),
        IdentType::Keyword(keyword) => Some(IdentifierInfo {
            ident_span: span.clone(),
            definition_span: None,
            symbol_id: None,
            reference_id: None,
            import: None,
            field: None,
            service_method: None,
            param: None,
            primitive: None,
            keyword: Some(keyword),
            actor: None,
        }),
        IdentType::Actor => {
            let actor = semantic.actor.as_ref()?;
            let name_span = actor.name_span.clone()?;
            Some(IdentifierInfo {
                ident_span: name_span,
                definition_span: Some(actor.span.clone()),
                symbol_id: None,
                reference_id: None,
                import: None,
                field: None,
                service_method: None,
                param: None,
                primitive: None,
                keyword: None,
                actor: Some(ActorIdentifier {
                    span: actor.span.clone(),
                    docs: actor.docs.clone(),
                    definition: actor.definition.clone(),
                }),
            })
        }
    }
}

fn ident_priority(ident: &IdentType) -> u8 {
    match ident {
        IdentType::Reference(_) => 5,
        IdentType::Primitive(_) => 4,
        IdentType::Keyword(_) => 4,
        IdentType::Actor => 3,
        IdentType::Field(_, FieldPart::Label) => 3,
        IdentType::ServiceMethod(_) => 3,
        IdentType::FuncParam(_) => 3,
        IdentType::Field(_, FieldPart::Type) => 2,
        IdentType::Binding(_) => 1,
    }
}
