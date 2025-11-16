use crate::lsp::{
    span::Span,
    symbol_table::{ImportKind, ReferenceId, SymbolId, SymbolTable},
};
use candid_parser::syntax::{
    Binding, Dec, IDLActorType, IDLProg, IDLType, IDLTypeWithSpan, TypeField,
};
use rust_lapper::{Interval, Lapper};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, SemanticError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentType {
    Binding(SymbolId),
    Reference(ReferenceId),
}
type IdentRangeLapper = Lapper<usize, IdentType>;

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
}

impl Semantic {}

#[derive(Debug)]
pub struct Ctx {
    env: im_rc::Vector<(String, Span)>,
    table: SymbolTable,
}

impl Ctx {
    fn find_symbol(&self, name: &str) -> Option<Span> {
        self.env
            .iter()
            .rev()
            .find_map(|(n, t)| if n == name { Some(t.clone()) } else { None })
    }

    fn declare_symbol<S: Into<String>>(&mut self, name: S, span: Span) -> SymbolId {
        let name = name.into();
        let symbol_id = self.table.add_symbol(span.clone());
        self.env.push_back((name, span));
        symbol_id
    }
}

pub fn analyze_program(ast: &IDLProg) -> Result<Semantic> {
    let table = SymbolTable::default();
    let env = im_rc::Vector::new();
    let mut ctx = Ctx { env, table };
    for dec in ast.decs.iter() {
        match dec {
            Dec::TypD(binding) => {
                ctx.declare_symbol(binding.id.clone(), binding.span.clone());
            }
            Dec::ImportType { path, span } => {
                ctx.table
                    .add_import(span.clone(), path.clone(), ImportKind::Type);
            }
            Dec::ImportServ { path, span } => {
                ctx.table
                    .add_import(span.clone(), path.clone(), ImportKind::Service);
            }
        }
    }

    for dec in ast.decs.iter() {
        analyze_dec(dec, &mut ctx)?;
    }

    if let Some(actor) = &ast.actor {
        analyze_actor(actor, &mut ctx)?;
    }

    let mut ident_range = IdentRangeLapper::new(vec![]);
    for (symbol_id, range) in ctx.table.symbol_id_to_span.iter_enumerated() {
        ident_range.insert(Interval {
            start: range.start,
            stop: range.end,
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
    Ok(Semantic {
        table: ctx.table,
        ident_range,
    })
}

fn analyze_dec(dec: &Dec, ctx: &mut Ctx) -> Result<()> {
    match dec {
        Dec::TypD(binding) => analyze_binding(binding, ctx),
        Dec::ImportType { .. } | Dec::ImportServ { .. } => Ok(()),
    }
}

fn analyze_binding(binding: &Binding, ctx: &mut Ctx) -> Result<()> {
    analyze_type(&binding.typ, ctx)
}

fn analyze_type(idl_type: &IDLTypeWithSpan, ctx: &mut Ctx) -> Result<()> {
    match &idl_type.kind {
        IDLType::PrimT(_) | IDLType::PrincipalT => {}
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
            for arg in func_type.args.iter() {
                analyze_type(arg, ctx)?;
            }
            for ret in func_type.rets.iter() {
                analyze_type(ret, ctx)?;
            }
        }
        IDLType::OptT(inner) | IDLType::VecT(inner) => {
            analyze_type(inner, ctx)?;
        }
        IDLType::RecordT(type_fields) | IDLType::VariantT(type_fields) => {
            analyze_type_fields(type_fields, ctx)?;
        }
        IDLType::ServT(bindings) => {
            for binding in bindings.iter() {
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
        analyze_type(&field.typ, ctx)?;
    }
    Ok(())
}

fn analyze_actor(actor: &IDLActorType, ctx: &mut Ctx) -> Result<()> {
    analyze_type(&actor.typ, ctx)
}
