use candid_parser::syntax::PrimType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDoc {
    pub definition: String,
    pub docs: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeywordDoc {
    Func,
    Opt,
    Principal,
    Record,
    Service,
    Type,
    Variant,
    Vec,
    Oneway,
    Query,
    CompositeQuery,
}

macro_rules! prim_doc {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/primitives/",
            $name,
            ".md"
        ))
    };
}

macro_rules! keyword_doc_file {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/keywords/",
            $name,
            ".md"
        ))
    };
}

fn primitive_name(kind: &PrimType) -> &'static str {
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

fn primitive_body(kind: &PrimType) -> &'static str {
    match kind {
        PrimType::Nat => prim_doc!("nat"),
        PrimType::Nat8 => prim_doc!("nat8"),
        PrimType::Nat16 => prim_doc!("nat16"),
        PrimType::Nat32 => prim_doc!("nat32"),
        PrimType::Nat64 => prim_doc!("nat64"),
        PrimType::Int => prim_doc!("int"),
        PrimType::Int8 => prim_doc!("int8"),
        PrimType::Int16 => prim_doc!("int16"),
        PrimType::Int32 => prim_doc!("int32"),
        PrimType::Int64 => prim_doc!("int64"),
        PrimType::Float32 => prim_doc!("float32"),
        PrimType::Float64 => prim_doc!("float64"),
        PrimType::Bool => prim_doc!("bool"),
        PrimType::Text => prim_doc!("text"),
        PrimType::Null => prim_doc!("null"),
        PrimType::Reserved => prim_doc!("reserved"),
        PrimType::Empty => prim_doc!("empty"),
    }
}

impl KeywordDoc {
    pub fn keyword(self) -> &'static str {
        match self {
            KeywordDoc::Func => "func",
            KeywordDoc::Opt => "opt",
            KeywordDoc::Principal => "principal",
            KeywordDoc::Record => "record",
            KeywordDoc::Service => "service",
            KeywordDoc::Type => "type",
            KeywordDoc::Variant => "variant",
            KeywordDoc::Vec => "vec",
            KeywordDoc::Oneway => "oneway",
            KeywordDoc::Query => "query",
            KeywordDoc::CompositeQuery => "composite_query",
        }
    }
}

fn keyword_body(kind: KeywordDoc) -> &'static str {
    match kind {
        KeywordDoc::Func => keyword_doc_file!("func"),
        KeywordDoc::Opt => keyword_doc_file!("opt"),
        KeywordDoc::Principal => keyword_doc_file!("principal"),
        KeywordDoc::Record => keyword_doc_file!("record"),
        KeywordDoc::Service => keyword_doc_file!("service"),
        KeywordDoc::Type => keyword_doc_file!("type"),
        KeywordDoc::Variant => keyword_doc_file!("variant"),
        KeywordDoc::Vec => keyword_doc_file!("vec"),
        KeywordDoc::Oneway => keyword_doc_file!("oneway"),
        KeywordDoc::Query => keyword_doc_file!("query"),
        KeywordDoc::CompositeQuery => keyword_doc_file!("composite_query"),
    }
}

pub fn primitive_doc(kind: &PrimType) -> String {
    let header = primitive_name(kind);
    let body = primitive_body(kind).trim();
    format!("```candid\n{header}\n```\n\n{body}")
}

pub fn blob_doc() -> String {
    let body = prim_doc!("blob").trim();
    format!("```candid\nblob\n```\n\n{body}")
}

pub fn keyword_doc(kind: KeywordDoc) -> Option<String> {
    let header = kind.keyword();
    let body = keyword_body(kind).trim();
    Some(format!("```candid\n{header}\n```\n\n{body}"))
}
