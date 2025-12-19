use crate::lsp::{
    span::Span,
    symbol_table::{ImportKind, ReferenceId, SymbolId, SymbolTable},
    type_display::{render_actor_declaration, render_binding, render_inline_type},
    type_docs::{KeywordDoc, TypeDoc},
};
use candid_parser::candid::types::internal::FuncMode;
use candid_parser::{
    candid::types::Label,
    syntax::{
        Binding, Dec, FuncType, IDLActorType, IDLMergedProg, IDLType, IDLTypeWithSpan, PrimType,
        TypeField,
    },
};
use oxc_index::IndexVec;
use ropey::Rope;
use rust_lapper::{Interval, Lapper};
use std::sync::Arc;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, SemanticError>;

oxc_index::define_index_type! {
    pub struct FieldId = u32;
    IMPL_RAW_CONVERSIONS = true;
}

oxc_index::define_index_type! {
    pub struct MethodId = u32;
    IMPL_RAW_CONVERSIONS = true;
}

oxc_index::define_index_type! {
    pub struct ParamId = u32;
    IMPL_RAW_CONVERSIONS = true;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPart {
    Label,
    Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamRole {
    Argument,
    Result,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentType {
    Binding(SymbolId),
    Reference(ReferenceId),
    Field(FieldId, FieldPart),
    ServiceMethod(MethodId),
    FuncParam(ParamId),
    Primitive(PrimitiveHover),
    Keyword(KeywordDoc),
    Actor,
}
type IdentRangeLapper = Lapper<usize, IdentType>;

#[derive(Debug, Clone)]
pub struct FieldMetadata {
    pub span: Span,
    pub label_span: Option<Span>,
    pub type_span: Option<Span>,
    pub docs: Option<Arc<str>>,
    pub parent_name: Option<Arc<str>>,
    pub label: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct MethodMetadata {
    pub span: Span,
    pub name_span: Option<Span>,
    pub type_span: Option<Span>,
    pub docs: Option<Arc<str>>,
    pub parent_name: Option<Arc<str>>,
    pub signature: Option<MethodSignature>,
}

#[derive(Debug, Clone)]
pub struct ParamMetadata {
    pub span: Span,
    pub name_span: Option<Span>,
    pub type_span: Span,
    pub role: ParamRole,
}

#[derive(Debug, Clone)]
pub struct LocalBinding {
    pub name: Arc<str>,
    pub span: Span,
    pub scope: Span,
    pub is_definition: bool,
}

#[derive(Debug, Clone)]
pub struct ActorMetadata {
    pub span: Span,
    pub name_span: Option<Span>,
    pub docs: Option<Arc<str>>,
    pub definition: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrimitiveHover {
    Prim(PrimType),
    Blob,
}

#[derive(Error, Debug)]
pub enum SemanticError {
    #[error("Undefined variable {name}")]
    UndefinedVariable { name: String, span: Span },
    #[error("Expect element type: {expect_ty}, but got {actual_ty}")]
    ImConsistentArrayType {
        expect_ty: String,
        actual_ty: String,
        span: Span,
    },
}

impl SemanticError {
    pub fn span(&self) -> Span {
        match self {
            SemanticError::UndefinedVariable { span, .. } => span.clone(),
            SemanticError::ImConsistentArrayType { span, .. } => span.clone(),
        }
    }
}

#[derive(Debug)]
pub struct Semantic {
    pub table: SymbolTable,
    pub ident_range: IdentRangeLapper,
    pub fields: IndexVec<FieldId, FieldMetadata>,
    pub service_methods: IndexVec<MethodId, MethodMetadata>,
    pub params: IndexVec<ParamId, ParamMetadata>,
    pub locals: Vec<LocalBinding>,
    pub symbol_ident_spans: IndexVec<SymbolId, Option<Span>>,
    pub symbol_ident_names: IndexVec<SymbolId, Option<Arc<str>>>,
    pub type_docs: IndexVec<SymbolId, Option<TypeDoc>>,
    pub primitive_spans: Vec<(Span, PrimitiveHover)>,
    pub keyword_spans: Vec<(Span, KeywordDoc)>,
    pub actor: Option<ActorMetadata>,
}

#[derive(Debug)]
pub struct Ctx<'a> {
    env: im_rc::Vector<(String, Span)>,
    table: SymbolTable,
    fields: IndexVec<FieldId, FieldMetadata>,
    service_methods: IndexVec<MethodId, MethodMetadata>,
    params: IndexVec<ParamId, ParamMetadata>,
    locals: Vec<LocalBinding>,
    rope: &'a Rope,
    symbol_ident_spans: IndexVec<SymbolId, Option<Span>>,
    symbol_ident_names: IndexVec<SymbolId, Option<Arc<str>>>,
    type_docs: IndexVec<SymbolId, Option<TypeDoc>>,
    primitive_spans: Vec<(Span, PrimitiveHover)>,
    keyword_spans: Vec<(Span, KeywordDoc)>,
    actor: Option<ActorMetadata>,
    type_name_stack: Vec<Option<Arc<str>>>,
    scope_stack: Vec<Span>,
}

impl<'a> Ctx<'a> {
    fn find_symbol(&self, name: &str) -> Option<Span> {
        self.env
            .iter()
            .rev()
            .find_map(|(n, t)| if n == name { Some(t.clone()) } else { None })
    }

    fn push_type_name(&mut self, name: Option<String>) {
        self.type_name_stack
            .push(name.map(|n| Arc::<str>::from(n.into_boxed_str())));
    }

    fn pop_type_name(&mut self) {
        self.type_name_stack.pop();
    }

    fn current_type_name(&self) -> Option<Arc<str>> {
        self.type_name_stack
            .iter()
            .rev()
            .find_map(|name| name.clone())
    }

    fn push_scope(&mut self, span: Span) {
        self.scope_stack.push(span);
    }

    fn pop_scope(&mut self) {
        self.scope_stack.pop();
    }

    fn current_scope(&self) -> Option<Span> {
        self.scope_stack.last().cloned()
    }

    fn declare_symbol<S: Into<String>>(&mut self, name: S, span: Span) -> SymbolId {
        let name = name.into();
        let symbol_id = self.table.add_symbol(span.clone());
        self.register_symbol_slot();
        if let Some(slot) = self.symbol_ident_names.get_mut(symbol_id) {
            *slot = Some(Arc::<str>::from(name.as_str()));
        }
        self.env.push_back((name, span));
        symbol_id
    }

    fn register_symbol_slot(&mut self) {
        self.symbol_ident_spans.push(None);
        self.symbol_ident_names.push(None);
        self.type_docs.push(None);
    }

    fn register_field(&mut self, field: &TypeField) {
        let label_span = compute_field_label_span(field, self.rope);
        let type_span = if field.typ.span.start < field.typ.span.end {
            Some(field.typ.span.clone())
        } else {
            None
        };

        let label_text = label_span
            .as_ref()
            .map(|span| {
                Arc::<str>::from(self.rope.slice(span.clone()).to_string().into_boxed_str())
            })
            .or_else(|| {
                field_label_name(field).map(|name| Arc::<str>::from(name.into_boxed_str()))
            });

        let metadata = FieldMetadata {
            span: field.span.clone(),
            label_span: label_span.clone(),
            type_span,
            docs: format_docs(&field.docs),
            parent_name: self.current_type_name(),
            label: label_text,
        };
        self.fields.push(metadata);

        if let (Some(scope), Some(label_span), Some(label_name)) =
            (self.current_scope(), label_span, field_label_name(field))
        {
            self.locals.push(LocalBinding {
                name: Arc::<str>::from(label_name.into_boxed_str()),
                span: label_span,
                scope,
                is_definition: false,
            });
        }
    }

    fn register_service_method(&mut self, binding: &Binding, parent_name: Option<Arc<str>>) {
        let name_span = compute_binding_ident_span(binding, self.rope);
        let type_span = if binding.typ.span.start < binding.typ.span.end {
            Some(binding.typ.span.clone())
        } else {
            None
        };
        let signature = match &binding.typ.kind {
            IDLType::FuncT(func) => Some(MethodSignature::from_func(func)),
            _ => None,
        };

        let metadata = MethodMetadata {
            span: binding.span.clone(),
            name_span,
            type_span,
            docs: format_docs(&binding.docs),
            parent_name,
            signature,
        };
        self.service_methods.push(metadata);
    }

    fn register_function_params(&mut self, func_type: &FuncType, func_span: &Span) {
        let mut cursor = args_region_start(self.rope, func_span);
        for arg in func_type.args.iter() {
            if cursor > arg.span.start {
                cursor = arg.span.start;
            }
            let search_span = cursor..arg.span.start;
            let name_span = compute_param_name_span(self.rope, &search_span);
            let span_start = name_span
                .as_ref()
                .map(|span| span.start)
                .unwrap_or(arg.span.start);
            let span = span_start..arg.span.end;
            let metadata = ParamMetadata {
                span: span.clone(),
                name_span: name_span.clone(),
                type_span: arg.span.clone(),
                role: ParamRole::Argument,
            };
            if let Some(name_span) = &name_span {
                let name = self.rope.slice(name_span.clone()).to_string();
                self.locals.push(LocalBinding {
                    name: Arc::<str>::from(name.into_boxed_str()),
                    span: name_span.clone(),
                    scope: func_span.clone(),
                    is_definition: true,
                });
            }
            self.params.push(metadata);
            cursor = arg.span.end;
        }
    }

    fn register_primitive(&mut self, span: &Span, kind: &PrimType) {
        if span.start >= span.end {
            return;
        }
        if let Some(prim_span) =
            find_identifier_span(self.rope, span.clone(), primitive_keyword(kind), false)
        {
            self.primitive_spans
                .push((prim_span, PrimitiveHover::Prim(kind.clone())));
        }
    }

    fn register_blob(&mut self, span: &Span) {
        if span.start >= span.end {
            return;
        }
        if let Some(blob_span) = find_identifier_span(self.rope, span.clone(), "blob", false) {
            self.primitive_spans.push((blob_span, PrimitiveHover::Blob));
        }
    }

    fn register_keyword(&mut self, span: Span, keyword: KeywordDoc) {
        self.register_keyword_within(span, keyword, false);
    }

    fn register_keyword_from_end(&mut self, span: Span, keyword: KeywordDoc) {
        self.register_keyword_within(span, keyword, true);
    }

    fn register_import_keyword(&mut self, span: Span, path: &str) {
        if span.start < span.end {
            let line_idx = self.rope.char_to_line(span.start);
            let line_start = self.rope.line_to_char(line_idx);
            let line_end = line_start + self.rope.line(line_idx).len_chars();
            let line_span = line_start..line_end;
            if let Some(keyword_span) =
                find_identifier_span(self.rope, line_span, KeywordDoc::Import.keyword(), false)
            {
                self.keyword_spans.push((keyword_span, KeywordDoc::Import));
                return;
            }
        }

        let needle = path;
        let quoted = format!("\"{path}\"");
        for line_idx in 0..self.rope.len_lines() {
            let line = self.rope.line(line_idx).to_string();
            if !line.contains("import") {
                continue;
            }
            if !line.contains(needle) && !line.contains(&quoted) {
                continue;
            }

            if let Some(idx) = line.find("import") {
                let line_start = self.rope.line_to_char(line_idx);
                let leading_chars = line[..idx].chars().count();
                let start = line_start + leading_chars;
                let len_chars = "import".chars().count();
                self.keyword_spans
                    .push((start..start + len_chars, KeywordDoc::Import));
                return;
            }
        }
    }

    fn register_import_keywords_from_text(&mut self) {
        let needle = KeywordDoc::Import.keyword();
        let needle_len = needle.chars().count();

        for line_idx in 0..self.rope.len_lines() {
            let line = self.rope.line(line_idx);
            let line_text = line.to_string();
            let trimmed = line_text.trim_start();
            if !trimmed.starts_with(needle) {
                continue;
            }

            let after = trimmed[needle.len()..].chars().next();
            if let Some(next) = after
                && !(next.is_whitespace() || next == '"')
            {
                continue;
            }

            let leading_chars = line_text.len() - trimmed.len();
            let line_start = self.rope.line_to_char(line_idx);
            let start = line_start + leading_chars;
            let span = start..start + needle_len;
            if self
                .keyword_spans
                .iter()
                .any(|(existing, kind)| *kind == KeywordDoc::Import && *existing == span)
            {
                continue;
            }
            self.keyword_spans.push((span, KeywordDoc::Import));
        }
    }

    fn register_keyword_within(&mut self, span: Span, keyword: KeywordDoc, from_end: bool) {
        if span.start >= span.end {
            return;
        }
        if let Some(keyword_span) =
            find_identifier_span(self.rope, span, keyword.keyword(), from_end)
        {
            self.keyword_spans.push((keyword_span, keyword));
        }
    }
}

pub fn analyze_program(ast: &IDLMergedProg, rope: &Rope) -> Result<Semantic> {
    let table = SymbolTable::default();
    let env = im_rc::Vector::new();
    let fields = IndexVec::new();
    let service_methods = IndexVec::new();
    let params = IndexVec::new();
    let mut ctx = Ctx {
        env,
        table,
        fields,
        service_methods,
        params,
        locals: Vec::new(),
        rope,
        symbol_ident_spans: IndexVec::new(),
        symbol_ident_names: IndexVec::new(),
        type_docs: IndexVec::new(),
        primitive_spans: Vec::new(),
        keyword_spans: Vec::new(),
        actor: None,
        type_name_stack: Vec::new(),
        scope_stack: Vec::new(),
    };
    for dec in ast.decs().iter() {
        match dec {
            Dec::TypD(binding) => {
                ctx.register_keyword(binding.span.clone(), KeywordDoc::Type);
                let symbol_id = ctx.declare_symbol(binding.id.clone(), binding.span.clone());
                if let Some(ident_span) = compute_binding_ident_span(binding, rope)
                    && let Some(slot) = ctx.symbol_ident_spans.get_mut(symbol_id)
                {
                    *slot = Some(ident_span);
                }
                if let Some(slot) = ctx.type_docs.get_mut(symbol_id) {
                    let rendered = render_binding(binding);
                    let doc_block = format_docs(&binding.docs);
                    *slot = Some(TypeDoc {
                        definition: Arc::<str>::from(rendered.into_boxed_str()),
                        docs: doc_block,
                    });
                }
            }
            Dec::ImportType { path, span } => {
                ctx.table
                    .add_import(span.clone(), path.clone(), ImportKind::Type);
                ctx.register_import_keyword(span.clone(), path);
                ctx.register_symbol_slot();
            }
            Dec::ImportServ { path, span } => {
                ctx.table
                    .add_import(span.clone(), path.clone(), ImportKind::Service);
                ctx.register_import_keyword(span.clone(), path);
                ctx.register_symbol_slot();
            }
        }
    }

    for dec in ast.decs().iter() {
        analyze_dec(dec, &mut ctx)?;
    }

    if let Some(actor) = &ast.resolve_actor().ok().flatten() {
        analyze_actor(actor, &mut ctx)?;
    }

    ctx.register_import_keywords_from_text();

    let mut ident_range = IdentRangeLapper::new(vec![]);
    for (symbol_id, range) in ctx.table.symbol_id_to_span.iter_enumerated() {
        let span = ctx
            .symbol_ident_spans
            .get(symbol_id)
            .and_then(|opt| opt.clone())
            .unwrap_or_else(|| range.clone());
        ident_range.insert(Interval {
            start: span.start,
            stop: span.end,
            val: IdentType::Binding(symbol_id),
        });
    }
    for (reference_id, reference) in ctx.table.reference_id_to_reference.iter_enumerated() {
        let range = &reference.span;
        ident_range.insert(Interval {
            start: range.start,
            stop: range.end,
            val: IdentType::Reference(reference_id),
        });
    }
    for (field_id, metadata) in ctx.fields.iter_enumerated() {
        if let Some(label_span) = &metadata.label_span {
            ident_range.insert(Interval {
                start: label_span.start,
                stop: label_span.end,
                val: IdentType::Field(field_id, FieldPart::Label),
            });
        }
        if let Some(type_span) = &metadata.type_span
            && type_span.start < type_span.end
            && metadata.label_span.as_ref() != Some(type_span)
        {
            ident_range.insert(Interval {
                start: type_span.start,
                stop: type_span.end,
                val: IdentType::Field(field_id, FieldPart::Type),
            });
        }
    }
    for (method_id, metadata) in ctx.service_methods.iter_enumerated() {
        if let Some(name_span) = &metadata.name_span {
            ident_range.insert(Interval {
                start: name_span.start,
                stop: name_span.end,
                val: IdentType::ServiceMethod(method_id),
            });
        }
    }
    for (param_id, metadata) in ctx.params.iter_enumerated() {
        if let Some(name_span) = &metadata.name_span {
            ident_range.insert(Interval {
                start: name_span.start,
                stop: name_span.end,
                val: IdentType::FuncParam(param_id),
            });
        }
    }
    for (span, kind) in ctx.primitive_spans.iter() {
        if span.start < span.end {
            ident_range.insert(Interval {
                start: span.start,
                stop: span.end,
                val: IdentType::Primitive(kind.clone()),
            });
        }
    }
    for (span, keyword) in ctx.keyword_spans.iter() {
        if span.start < span.end {
            ident_range.insert(Interval {
                start: span.start,
                stop: span.end,
                val: IdentType::Keyword(*keyword),
            });
        }
    }
    if let Some(actor) = &ctx.actor
        && let Some(name_span) = &actor.name_span
        && name_span.start < name_span.end
    {
        ident_range.insert(Interval {
            start: name_span.start,
            stop: name_span.end,
            val: IdentType::Actor,
        });
    }
    Ok(Semantic {
        table: ctx.table,
        ident_range,
        fields: ctx.fields,
        service_methods: ctx.service_methods,
        params: ctx.params,
        locals: ctx.locals,
        symbol_ident_spans: ctx.symbol_ident_spans,
        symbol_ident_names: ctx.symbol_ident_names,
        type_docs: ctx.type_docs,
        primitive_spans: ctx.primitive_spans,
        keyword_spans: ctx.keyword_spans,
        actor: ctx.actor,
    })
}

fn analyze_dec(dec: &Dec, ctx: &mut Ctx) -> Result<()> {
    match dec {
        Dec::TypD(binding) => {
            ctx.push_type_name(Some(binding.id.clone()));
            let result = analyze_binding(binding, ctx);
            ctx.pop_type_name();
            result
        }
        Dec::ImportType { .. } | Dec::ImportServ { .. } => Ok(()),
    }
}

fn analyze_binding(binding: &Binding, ctx: &mut Ctx) -> Result<()> {
    analyze_type(&binding.typ, ctx)
}

fn analyze_type(idl_type: &IDLTypeWithSpan, ctx: &mut Ctx) -> Result<()> {
    match &idl_type.kind {
        IDLType::PrimT(kind) => {
            ctx.register_primitive(&idl_type.span, kind);
        }
        IDLType::PrincipalT => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Principal);
        }
        IDLType::VarT(name) => {
            let span = match ctx.find_symbol(name) {
                Some(span) => span,
                None => {
                    return Err(SemanticError::UndefinedVariable {
                        name: name.to_owned(),
                        span: idl_type.span.clone(),
                    });
                }
            };
            if let Some(symbol_id) = ctx.table.span_to_symbol_id.get(&span).copied() {
                ctx.table
                    .add_reference(idl_type.span.clone(), Some(symbol_id));
            }
        }
        IDLType::FuncT(func_type) => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Func);
            ctx.register_function_params(func_type, &idl_type.span);
            for mode in func_type.modes.iter() {
                match mode {
                    FuncMode::Query => {
                        ctx.register_keyword_from_end(idl_type.span.clone(), KeywordDoc::Query)
                    }
                    FuncMode::CompositeQuery => ctx.register_keyword_from_end(
                        idl_type.span.clone(),
                        KeywordDoc::CompositeQuery,
                    ),
                    FuncMode::Oneway => {
                        ctx.register_keyword_from_end(idl_type.span.clone(), KeywordDoc::Oneway)
                    }
                }
            }
            for arg in func_type.args.iter() {
                analyze_type(arg, ctx)?;
            }
            for ret in func_type.rets.iter() {
                analyze_type(ret, ctx)?;
            }
        }
        IDLType::OptT(inner) => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Opt);
            analyze_type(inner, ctx)?;
        }
        IDLType::VecT(inner) => {
            if is_blob(&idl_type.span, ctx.rope) {
                ctx.register_blob(&idl_type.span);
            } else {
                ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Vec);
            }
            analyze_type(inner, ctx)?;
        }
        IDLType::RecordT(type_fields) => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Record);
            ctx.push_scope(idl_type.span.clone());
            analyze_type_fields(type_fields, ctx)?;
            ctx.pop_scope();
        }
        IDLType::VariantT(type_fields) => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Variant);
            ctx.push_scope(idl_type.span.clone());
            analyze_type_fields(type_fields, ctx)?;
            ctx.pop_scope();
        }
        IDLType::ServT(bindings) => {
            ctx.register_keyword(idl_type.span.clone(), KeywordDoc::Service);
            let parent_name = ctx.current_type_name();
            for binding in bindings.iter() {
                ctx.register_service_method(binding, parent_name.clone());
                analyze_binding(binding, ctx)?;
            }
        }
        IDLType::ClassT(args, ret) => {
            for arg in args.iter() {
                analyze_type(arg, ctx)?;
            }
            analyze_type(ret, ctx)?;
        }
    }
    Ok(())
}

fn analyze_type_fields(fields: &[TypeField], ctx: &mut Ctx) -> Result<()> {
    for field in fields.iter() {
        ctx.register_field(field);
        analyze_type(&field.typ, ctx)?;
    }
    Ok(())
}

fn analyze_actor(actor: &IDLActorType, ctx: &mut Ctx) -> Result<()> {
    ctx.register_keyword(actor.span.clone(), KeywordDoc::Service);
    let docs = format_docs(&actor.docs);
    let name_span = compute_actor_name_span(actor, ctx.rope);
    let name_text = name_span
        .as_ref()
        .map(|span| ctx.rope.slice(span.clone()).to_string());
    let definition = render_actor_declaration(name_text.as_deref(), &actor.typ)
        .map(|text| Arc::<str>::from(text.into_boxed_str()));
    ctx.actor = Some(ActorMetadata {
        span: actor.span.clone(),
        name_span,
        docs,
        definition,
    });
    ctx.push_type_name(name_text);
    let result = analyze_type(&actor.typ, ctx);
    ctx.pop_type_name();
    result
}

fn compute_actor_name_span(actor: &IDLActorType, rope: &Rope) -> Option<Span> {
    let colon_pos = find_char_in_span(rope, &actor.span, ':')?;
    let search_span = actor.span.start..colon_pos;
    let text = rope.slice(search_span.clone()).to_string();
    let trimmed = text.trim_start();
    let without_keyword = trimmed.strip_prefix("service")?.trim_start();
    if without_keyword.is_empty() {
        return None;
    }
    let name = without_keyword.split_whitespace().next()?;
    if name.is_empty() {
        return None;
    }
    find_identifier_span(rope, search_span, name, false)
}

pub(crate) fn compute_binding_ident_span(binding: &Binding, rope: &Rope) -> Option<Span> {
    if binding.id.is_empty() {
        return None;
    }

    let start = binding.span.start;
    let mut end = binding.typ.span.start;
    if end <= start {
        end = binding.span.end;
    }
    if end <= start {
        return None;
    }

    find_identifier_span(rope, start..end, &binding.id, true)
}

pub(crate) fn compute_field_label_span(field: &TypeField, rope: &Rope) -> Option<Span> {
    let label_text = match &field.label {
        Label::Named(name) => name.clone(),
        Label::Id(id) | Label::Unnamed(id) => id.to_string(),
    };

    if label_text.is_empty() {
        return None;
    }

    let start = field.span.start;
    let mut end = field.typ.span.start;
    if end <= start {
        end = field.span.end;
    }
    if end <= start {
        return None;
    }

    find_identifier_span(rope, start..end, &label_text, true)
}

fn field_label_name(field: &TypeField) -> Option<String> {
    match &field.label {
        Label::Named(name) => Some(name.clone()),
        Label::Id(id) | Label::Unnamed(id) => Some(id.to_string()),
    }
}

fn args_region_start(rope: &Rope, span: &Span) -> usize {
    match find_char_in_span(rope, span, '(') {
        Some(pos) => pos + 1,
        None => span.start,
    }
}

fn find_char_in_span(rope: &Rope, span: &Span, ch: char) -> Option<usize> {
    if span.start >= span.end {
        return None;
    }
    let text = rope.slice(span.start..span.end).to_string();
    let byte_idx = text.find(ch)?;
    let char_offset = text[..byte_idx].chars().count();
    Some(span.start + char_offset)
}

fn compute_param_name_span(rope: &Rope, search_span: &Span) -> Option<Span> {
    if search_span.start >= search_span.end {
        return None;
    }
    let text = rope.slice(search_span.start..search_span.end).to_string();
    let colon_idx = text.rfind(':')?;
    let before = &text[..colon_idx];
    let candidate = before.rsplit(['(', ',']).next().unwrap_or(before);
    let name = candidate.trim();
    if name.is_empty() {
        return None;
    }
    find_identifier_span(rope, search_span.clone(), name, true)
}

fn span_text_eq(rope: &Rope, span: &Span, needle: &str) -> bool {
    if span.start >= span.end {
        return false;
    }
    rope.slice(span.start..span.end).to_string().trim() == needle
}

fn is_blob(span: &Span, rope: &Rope) -> bool {
    span_text_eq(rope, span, "blob")
}

fn find_identifier_span(rope: &Rope, span: Span, needle: &str, from_end: bool) -> Option<Span> {
    if needle.is_empty() || span.start >= span.end {
        return None;
    }

    let text = rope.slice(span.start..span.end).to_string();
    let idx = if from_end {
        text.rmatch_indices(needle).next()?.0
    } else {
        text.find(needle)?
    };

    let leading_chars = text[..idx].chars().count();
    let start = span.start + leading_chars;
    let len_chars = needle.chars().count();
    Some(start..start + len_chars)
}

fn primitive_keyword(kind: &PrimType) -> &'static str {
    match kind {
        PrimType::Nat => "nat",
        PrimType::Nat8 => "nat8",
        PrimType::Nat16 => "nat16",
        PrimType::Nat32 => "nat32",
        PrimType::Nat64 => "nat64",
        PrimType::Int => "int",
        PrimType::Int8 => "int8",
        PrimType::Int16 => "int16",
        PrimType::Int32 => "int32",
        PrimType::Int64 => "int64",
        PrimType::Float32 => "float32",
        PrimType::Float64 => "float64",
        PrimType::Bool => "bool",
        PrimType::Text => "text",
        PrimType::Null => "null",
        PrimType::Reserved => "reserved",
        PrimType::Empty => "empty",
    }
}

fn format_docs(docs: &[String]) -> Option<Arc<str>> {
    if docs.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for doc in docs {
        let trimmed = doc.trim().trim_start_matches('/').trim();
        if trimmed.is_empty() {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    if lines.is_empty() {
        None
    } else {
        Some(Arc::<str>::from(
            annotate_code_fences(&lines.join("\n")).into_boxed_str(),
        ))
    }
}

fn annotate_code_fences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_code = false;
    while i < len {
        if i + 3 <= len && &bytes[i..i + 3] == b"```" {
            result.push_str("```");
            i += 3;
            if !in_code {
                result.push_str("candid");
                in_code = true;
            } else {
                in_code = false;
            }
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        if ch == '\n' {
            if in_code {
                result.push('\n');
            } else {
                result.push_str("  \n");
            }
        } else if ch == '\r' {
            // Preserve Windows line endings but still enforce Markdown break
            if in_code {
                result.push('\r');
            } else {
                result.push_str("  \r");
            }
        } else {
            result.push(ch);
        }
        i += ch.len_utf8();
    }
    result
}

fn flatten_type_text(ty: &IDLTypeWithSpan) -> Arc<str> {
    let rendered = render_inline_type(ty);
    let compact = collapse_whitespace(&rendered);
    Arc::<str>::from(compact.into_boxed_str())
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
#[derive(Debug, Clone)]
pub struct MethodSignature {
    pub args: Vec<Arc<str>>,
    pub rets: Vec<Arc<str>>,
    pub modes: Vec<FuncMode>,
}

impl MethodSignature {
    pub fn from_func(func: &FuncType) -> Self {
        let args = func.args.iter().map(flatten_type_text).collect::<Vec<_>>();
        let rets = func.rets.iter().map(flatten_type_text).collect::<Vec<_>>();
        Self {
            args,
            rets,
            modes: func.modes.clone(),
        }
    }
}
