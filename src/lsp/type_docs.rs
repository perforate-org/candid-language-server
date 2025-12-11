use candid_parser::syntax::PrimType;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDoc {
    pub definition: Arc<str>,
    pub docs: Option<Arc<str>>,
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
    Import,
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

pub fn primitive_name(kind: &PrimType) -> &'static str {
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
            KeywordDoc::Import => "import",
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
        KeywordDoc::Import => keyword_doc_file!("import"),
    }
}

pub const PRIMITIVE_KINDS: [PrimType; 17] = [
    PrimType::Nat,
    PrimType::Nat8,
    PrimType::Nat16,
    PrimType::Nat32,
    PrimType::Nat64,
    PrimType::Int,
    PrimType::Int8,
    PrimType::Int16,
    PrimType::Int32,
    PrimType::Int64,
    PrimType::Float32,
    PrimType::Float64,
    PrimType::Bool,
    PrimType::Text,
    PrimType::Null,
    PrimType::Reserved,
    PrimType::Empty,
];

pub const KEYWORD_KINDS: [KeywordDoc; 12] = [
    KeywordDoc::Func,
    KeywordDoc::Opt,
    KeywordDoc::Principal,
    KeywordDoc::Record,
    KeywordDoc::Service,
    KeywordDoc::Type,
    KeywordDoc::Variant,
    KeywordDoc::Vec,
    KeywordDoc::Oneway,
    KeywordDoc::Query,
    KeywordDoc::CompositeQuery,
    KeywordDoc::Import,
];

static PRIMITIVE_DOCS: OnceLock<[String; 17]> = OnceLock::new();
static KEYWORD_DOCS: OnceLock<[String; 12]> = OnceLock::new();
static BLOB_DOC: OnceLock<String> = OnceLock::new();

pub fn primitive_doc(kind: &PrimType) -> &'static str {
    let docs = PRIMITIVE_DOCS.get_or_init(build_primitive_docs);
    docs[primitive_index(kind)].as_str()
}

pub fn blob_doc() -> &'static str {
    BLOB_DOC
        .get_or_init(|| {
            let body = prim_doc!("blob").trim();
            format!("```candid\nblob\n```\n\n{body}")
        })
        .as_str()
}

pub fn keyword_doc(kind: KeywordDoc) -> Option<&'static str> {
    let docs = KEYWORD_DOCS.get_or_init(build_keyword_docs);
    Some(docs[keyword_index(kind)].as_str())
}

fn build_primitive_docs() -> [String; 17] {
    std::array::from_fn(|idx| {
        let kind = &PRIMITIVE_KINDS[idx];
        let header = primitive_name(kind);
        let body = primitive_body(kind).trim();
        format!("```candid\n{header}\n```\n\n{body}")
    })
}

fn primitive_index(kind: &PrimType) -> usize {
    match kind {
        PrimType::Nat => 0,
        PrimType::Nat8 => 1,
        PrimType::Nat16 => 2,
        PrimType::Nat32 => 3,
        PrimType::Nat64 => 4,
        PrimType::Int => 5,
        PrimType::Int8 => 6,
        PrimType::Int16 => 7,
        PrimType::Int32 => 8,
        PrimType::Int64 => 9,
        PrimType::Float32 => 10,
        PrimType::Float64 => 11,
        PrimType::Bool => 12,
        PrimType::Text => 13,
        PrimType::Null => 14,
        PrimType::Reserved => 15,
        PrimType::Empty => 16,
    }
}

fn build_keyword_docs() -> [String; 12] {
    std::array::from_fn(|idx| {
        let kind = KEYWORD_KINDS[idx];
        let header = kind.keyword();
        let body = keyword_body(kind).trim();
        format!("```candid\n{header}\n```\n\n{body}")
    })
}

fn keyword_index(kind: KeywordDoc) -> usize {
    match kind {
        KeywordDoc::Func => 0,
        KeywordDoc::Opt => 1,
        KeywordDoc::Principal => 2,
        KeywordDoc::Record => 3,
        KeywordDoc::Service => 4,
        KeywordDoc::Type => 5,
        KeywordDoc::Variant => 6,
        KeywordDoc::Vec => 7,
        KeywordDoc::Oneway => 8,
        KeywordDoc::Query => 9,
        KeywordDoc::CompositeQuery => 10,
        KeywordDoc::Import => 11,
    }
}

pub fn primitive_kinds() -> &'static [PrimType; 17] {
    &PRIMITIVE_KINDS
}

pub fn keyword_kinds() -> &'static [KeywordDoc; 12] {
    &KEYWORD_KINDS
}
