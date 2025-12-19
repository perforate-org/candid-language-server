use candid_language_server::{
    candid_lang::{ParserResult, parse},
    lsp::format::format_program,
};
use ropey::Rope;

#[test]
fn formatting_returns_text_edits() {
    let text = "service : { method : (text) -> (text) query; }";
    let rope = Rope::from_str(text);
    let ParserResult { ast, .. } = parse(text);
    let ast = ast.expect("parsed AST");

    let edits = format_program(&ast, &rope).expect("edits");
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];

    // Check if new text is formatted.
    // pretty_print typically formats nicely.
    // "service : { method : (text) -> (text) query }"
    // might become:
    // service : {
    //   method : (text) -> (text) query;
    // }
    // or similar.
    assert!(edit.new_text.contains("service : {"));
    assert_ne!(
        edit.new_text, text,
        "formatted text should differ from ugly input"
    );
}

#[test]
fn formatting_preserves_imports_and_service_name() {
    let text = "import \"./shared.did\";\n\nservice Api : { method : (text) -> (); }";
    let rope = Rope::from_str(text);
    let ParserResult { ast, .. } = parse(text);
    let ast = ast.expect("parsed AST");

    let edits = format_program(&ast, &rope).expect("edits");
    let edit = &edits[0];

    assert!(edit.new_text.contains("import \"./shared.did\";"));
    assert!(edit.new_text.contains("service Api :"));
    let import_pos = edit
        .new_text
        .find("import \"./shared.did\";")
        .expect("import should exist");
    let service_pos = edit
        .new_text
        .find("service Api :")
        .expect("service name should exist");
    assert!(
        import_pos < service_pos,
        "import should remain before service declaration"
    );
}

#[test]
fn formatting_preserves_orphan_comment_lines() {
    let text = "// orphan comment\n\nservice : { method : () -> (); }";
    let rope = Rope::from_str(text);
    let ParserResult { ast, .. } = parse(text);
    let ast = ast.expect("parsed AST");

    let edits = format_program(&ast, &rope).expect("edits");
    let edit = &edits[0];

    assert!(
        edit.new_text.contains("// orphan comment"),
        "orphan comment should be preserved"
    );
    let comment_pos = edit
        .new_text
        .find("// orphan comment")
        .expect("comment should exist");
    let service_pos = edit.new_text.find("service").expect("service should exist");
    assert!(
        comment_pos < service_pos,
        "comment should remain before service"
    );
}
