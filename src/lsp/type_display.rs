use candid_parser::{
    candid::types::{FuncMode, Label},
    syntax::{Binding, FuncType, IDLType, IDLTypeWithSpan, PrimType, TypeField},
};
use std::fmt::Write;

const INDENT_STR: &str = "  ";

pub fn render_binding(binding: &Binding) -> String {
    let mut buf = String::new();
    buf.push_str("type ");
    buf.push_str(&binding.id);
    buf.push_str(" = ");
    render_type(&binding.typ, 0, &mut buf);
    buf
}

pub fn render_inline_type(ty: &IDLTypeWithSpan) -> String {
    let mut buf = String::new();
    render_type(ty, 0, &mut buf);
    buf
}

fn render_type(ty: &IDLTypeWithSpan, indent: usize, buf: &mut String) {
    render_type_kind(&ty.kind, indent, buf)
}

fn render_type_kind(ty: &IDLType, indent: usize, buf: &mut String) {
    match ty {
        IDLType::PrimT(kind) => buf.push_str(prim_to_str(kind)),
        IDLType::VarT(name) => buf.push_str(name),
        IDLType::PrincipalT => buf.push_str("principal"),
        IDLType::OptT(inner) => {
            buf.push_str("opt ");
            render_type(inner, indent, buf);
        }
        IDLType::VecT(inner) => {
            if matches!(inner.kind, IDLType::PrimT(PrimType::Nat8)) {
                buf.push_str("blob");
            } else {
                buf.push_str("vec ");
                render_type(inner, indent, buf);
            }
        }
        IDLType::RecordT(fields) => render_record(fields, indent, buf, ty.is_tuple()),
        IDLType::VariantT(fields) => render_variant(fields, indent, buf),
        IDLType::FuncT(func) => render_function(func, indent, buf, true),
        IDLType::ServT(methods) => render_service(methods, indent, buf),
        IDLType::ClassT(args, ret) => render_class(args, ret, indent, buf),
    }
}

fn render_record(fields: &[TypeField], indent: usize, buf: &mut String, is_tuple: bool) {
    if is_tuple {
        buf.push_str("record { ");
        for (idx, field) in fields.iter().enumerate() {
            if idx > 0 {
                buf.push_str("; ");
            }
            render_type(&field.typ, indent + 1, buf);
        }
        buf.push_str(" }");
        return;
    }

    buf.push_str("record {\n");
    for field in fields {
        push_indent(buf, indent + 1);
        render_label(&field.label, buf);
        buf.push_str(" : ");
        render_type(&field.typ, indent + 1, buf);
        buf.push_str(";\n");
    }
    push_indent(buf, indent);
    buf.push('}');
}

fn render_variant(fields: &[TypeField], indent: usize, buf: &mut String) {
    buf.push_str("variant {\n");
    for field in fields {
        push_indent(buf, indent + 1);
        render_label(&field.label, buf);
        if !matches!(field.typ.kind, IDLType::PrimT(PrimType::Null)) {
            buf.push_str(" : ");
            render_type(&field.typ, indent + 1, buf);
        }
        buf.push_str(";\n");
    }
    push_indent(buf, indent);
    buf.push('}');
}

fn render_function(func: &FuncType, indent: usize, buf: &mut String, include_keyword: bool) {
    if include_keyword {
        buf.push_str("func ");
    }
    render_args(&func.args, indent, buf);
    buf.push_str(" -> ");
    render_args(&func.rets, indent, buf);
    for mode in &func.modes {
        buf.push(' ');
        buf.push_str(match mode {
            FuncMode::Oneway => "oneway",
            FuncMode::Query => "query",
            FuncMode::CompositeQuery => "composite_query",
        });
    }
}

fn render_args(args: &[IDLTypeWithSpan], indent: usize, buf: &mut String) {
    buf.push('(');
    for (idx, arg) in args.iter().enumerate() {
        if idx > 0 {
            buf.push_str(", ");
        }
        render_type(arg, indent + 1, buf);
    }
    buf.push(')');
}

fn render_service(methods: &[Binding], indent: usize, buf: &mut String) {
    buf.push_str("service ");
    render_service_body(methods, indent, buf);
}

fn render_service_body(methods: &[Binding], indent: usize, buf: &mut String) {
    buf.push_str("{\n");
    for method in methods {
        push_indent(buf, indent + 1);
        buf.push_str(&method.id);
        buf.push_str(" : ");
        match &method.typ.kind {
            IDLType::FuncT(func) => render_function(func, indent + 1, buf, false),
            IDLType::VarT(name) => buf.push_str(name),
            other => render_type_kind(other, indent + 1, buf),
        }
        buf.push_str(";\n");
    }
    push_indent(buf, indent);
    buf.push('}');
}

pub fn render_actor_declaration(name: Option<&str>, ty: &IDLTypeWithSpan) -> Option<String> {
    if let IDLType::ServT(methods) = &ty.kind {
        let mut buf = String::new();
        buf.push_str("service");
        if let Some(name) = name {
            buf.push(' ');
            buf.push_str(name);
        }
        buf.push_str(" : ");
        render_service_body(methods, 0, &mut buf);
        Some(buf)
    } else {
        None
    }
}

fn render_class(args: &[IDLTypeWithSpan], ret: &IDLTypeWithSpan, indent: usize, buf: &mut String) {
    render_args(args, indent, buf);
    buf.push_str(" -> ");
    match &ret.kind {
        IDLType::ServT(methods) => render_service(methods, indent, buf),
        IDLType::VarT(_) | IDLType::ClassT(_, _) | IDLType::FuncT(_) => {
            render_type(ret, indent, buf)
        }
        other => render_type_kind(other, indent, buf),
    }
}

fn render_label(label: &Label, buf: &mut String) {
    match label {
        Label::Named(name) => {
            if should_quote_label(name) {
                buf.push('"');
                buf.push_str(name);
                buf.push('"');
            } else {
                buf.push_str(name);
            }
        }
        Label::Id(id) | Label::Unnamed(id) => {
            let _ = write!(buf, "{id}");
        }
    }
}

fn push_indent(buf: &mut String, level: usize) {
    for _ in 0..level {
        buf.push_str(INDENT_STR);
    }
}

fn prim_to_str(prim: &PrimType) -> &'static str {
    match prim {
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

fn should_quote_label(name: &str) -> bool {
    matches!(
        name,
        "nat"
            | "nat8"
            | "nat16"
            | "nat32"
            | "nat64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "float32"
            | "float64"
            | "bool"
            | "text"
            | "null"
            | "reserved"
            | "empty"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn binding(name: &str, ty_src: &str) -> Binding {
        Binding {
            id: name.to_string(),
            typ: IDLTypeWithSpan::from_str(ty_src).expect("valid type"),
            docs: vec![],
            span: 0..0,
        }
    }

    #[test]
    fn renders_record_binding() {
        let binding = binding("PaperSummary", "record { id : text; title : text }");
        let rendered = render_binding(&binding);
        assert_eq!(
            rendered,
            "type PaperSummary = record {\n  id : text;\n  title : text;\n}"
        );
    }

    #[test]
    fn renders_variant_binding() {
        let binding = binding("Citation", "variant { Url : text; Other : text; Paper }");
        let rendered = render_binding(&binding);
        assert_eq!(
            rendered,
            "type Citation = variant {\n  Url : text;\n  Paper;\n  Other : text;\n}"
        );
    }
}
