use crate::lsp::{
    semantic_analyze::{
        FieldId, FieldPart, IdentType, MethodId, ParamId, PrimitiveHover, Semantic,
    },
    span::Span,
    symbol_table::{ReferenceId, SymbolId},
    type_docs::KeywordDoc,
};

/// Captures information about the identifier located at a given position.
#[derive(Debug, Clone)]
pub struct IdentifierInfo {
    pub ident_span: Span,
    pub definition_span: Option<Span>,
    pub symbol_id: Option<SymbolId>,
    pub reference_id: Option<ReferenceId>,
    pub field: Option<FieldIdentifier>,
    pub service_method: Option<MethodIdentifier>,
    pub param: Option<ParamIdentifier>,
    pub primitive: Option<PrimitiveHover>,
    pub keyword: Option<KeywordDoc>,
    pub actor: Option<ActorIdentifier>,
}

#[derive(Debug, Clone)]
pub struct FieldIdentifier {
    pub id: FieldId,
    pub role: FieldRole,
}

#[derive(Debug, Clone)]
pub struct MethodIdentifier {
    pub id: MethodId,
}

#[derive(Debug, Clone)]
pub struct ParamIdentifier {
    pub id: ParamId,
}

#[derive(Debug, Clone)]
pub struct ActorIdentifier;

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

            Some(IdentifierInfo {
                ident_span: span,
                definition_span: Some(fallback_span),
                symbol_id: Some(symbol_id),
                reference_id: None,
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

            Some(IdentifierInfo {
                ident_span: reference.span.clone(),
                definition_span,
                symbol_id,
                reference_id: Some(reference_id),
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
                field: Some(FieldIdentifier {
                    id: field_id,
                    role: match part {
                        FieldPart::Label => FieldRole::Label,
                        FieldPart::Type => FieldRole::Type,
                    },
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
                field: None,
                service_method: Some(MethodIdentifier { id: method_id }),
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
                field: None,
                service_method: None,
                param: Some(ParamIdentifier { id: param_id }),
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
                field: None,
                service_method: None,
                param: None,
                primitive: None,
                keyword: None,
                actor: Some(ActorIdentifier),
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
