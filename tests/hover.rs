use candid_language_server::{
    candid_lang::{ParserResult, parse},
    lsp::{
        hover::hover_contents,
        navigation::lookup_identifier,
        semantic_analyze::analyze_program,
        type_docs::{KeywordDoc, keyword_doc},
    },
};
use ropey::Rope;
use tower_lsp_server::ls_types::HoverContents;

fn load_fixture() -> (String, Rope) {
    let text = include_str!("data/hover_sample.did").to_string();
    let rope = Rope::from_str(&text);
    (text, rope)
}

#[test]
fn hover_displays_type_definition() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("Foo").expect("Foo span");
    let info = lookup_identifier(&semantic, offset).expect("lookup Foo");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup.value.contains("type Foo = record"),
        "type definition missing: {}",
        markup.value
    );
}

#[test]
fn hover_displays_primitive_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("nat32").expect("nat32 span");
    let info = lookup_identifier(&semantic, offset).expect("lookup nat");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup.value.starts_with("```candid\nnat32"),
        "primitive header missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("Unsigned 32-bit integers"),
        "primitive description missing: {}",
        markup.value
    );
}

#[test]
fn hover_variant_without_type_does_not_add_null_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("Empty").expect("Empty span");
    let info = lookup_identifier(&semantic, offset).expect("lookup Empty");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        !markup.value.contains("```candid\nnull"),
        "unexpected null snippet: {}",
        markup.value
    );
    assert!(
        !markup.value.contains("# null"),
        "null docs should not appear for implicit variant fields: {}",
        markup.value
    );
}

#[test]
fn hover_field_includes_parent_name() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("value : nat32").expect("field span");
    let info = lookup_identifier(&semantic, offset).expect("lookup field");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup.value.contains("```candid\nFoo\n```"),
        "parent header missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("```candid\nvalue : nat32"),
        "field snippet missing: {}",
        markup.value
    );
}

#[test]
fn hover_keyword_vec_displays_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("vec Foo").expect("vec span");
    let info = lookup_identifier(&semantic, offset).expect("lookup vec");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    let expected_doc = keyword_doc(KeywordDoc::Vec).expect("vec doc");
    assert_eq!(
        markup.value.trim(),
        expected_doc.trim(),
        "vec hover should render keyword docs only"
    );
}

#[test]
fn hover_keyword_type_displays_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("type Foo").expect("type keyword span");
    let info = lookup_identifier(&semantic, offset).expect("lookup type");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    let expected_doc = keyword_doc(KeywordDoc::Type).expect("type doc");
    assert_eq!(
        markup.value.trim(),
        expected_doc.trim(),
        "type keyword hover should render keyword docs only"
    );
}

#[test]
fn hover_keyword_service_displays_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("service Api").expect("service keyword span");
    let info = lookup_identifier(&semantic, offset).expect("lookup service");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    let expected_doc = keyword_doc(KeywordDoc::Service).expect("service doc");
    assert_eq!(
        markup.value.trim(),
        expected_doc.trim(),
        "service keyword hover should render keyword docs only"
    );
}

#[test]
fn hover_actor_name_displays_docs() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let decl = "service Api";
    let offset = text
        .find(decl)
        .map(|idx| idx + "service ".len())
        .expect("actor name span");
    let info = lookup_identifier(&semantic, offset).expect("lookup actor name");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup.value.contains("```candid\nservice Api :"),
        "actor snippet missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("Demo API for hover tests."),
        "actor docs missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("---"),
        "actor docs should be separated: {}",
        markup.value
    );
}

#[test]
fn hover_keyword_query_displays_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("query").expect("query span");
    let info = lookup_identifier(&semantic, offset).expect("lookup query");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    let expected_doc = keyword_doc(KeywordDoc::Query).expect("query doc");
    assert_eq!(
        markup.value.trim(),
        expected_doc.trim(),
        "query keyword hover should render keyword docs only"
    );
}

#[test]
fn hover_keyword_composite_query_displays_doc() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("composite_query").expect("composite_query span");
    let info = lookup_identifier(&semantic, offset).expect("lookup composite_query");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    let expected_doc = keyword_doc(KeywordDoc::CompositeQuery).expect("composite_query doc");
    assert_eq!(
        markup.value.trim(),
        expected_doc.trim(),
        "composite_query keyword hover should render keyword docs only"
    );
}

#[test]
fn hover_service_method_displays_signature() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("get_value").expect("service method span");
    let info = lookup_identifier(&semantic, offset).expect("lookup service method");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup
            .value
            .contains("```candid\nget_value : () -> (Foo) query"),
        "service method signature missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("```candid\nApi\n```"),
        "method parent header missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("Returns the stored Foo value."),
        "service method docs missing: {}",
        markup.value
    );
    assert!(
        markup.value.contains("---"),
        "service method docs should be separated: {}",
        markup.value
    );
}

#[test]
fn hover_service_parameter_displays_binding() {
    let (text, rope) = load_fixture();
    let ParserResult { ast, .. } = parse(&text);
    let ast = ast.expect("parsed AST");
    let semantic = analyze_program(&ast, &rope).expect("semantic");

    let offset = text.find("value : Foo").expect("parameter span");
    let info = lookup_identifier(&semantic, offset).expect("lookup parameter");

    let hover = hover_contents(&rope, &semantic, &info).expect("hover result");
    let HoverContents::Markup(markup) = hover else {
        panic!("expected markup");
    };
    assert!(
        markup.value.contains("```candid\nvalue : Foo"),
        "parameter snippet missing: {}",
        markup.value
    );
}
