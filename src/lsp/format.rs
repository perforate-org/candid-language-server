use crate::CandidLanguageServer;
use candid_parser::syntax::{IDLMergedProg, pretty_print};
use ropey::Rope;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{DocumentFormattingParams, Position, Range, TextEdit};

pub async fn format(
    server: &CandidLanguageServer,
    params: DocumentFormattingParams,
) -> Result<Option<Vec<TextEdit>>> {
    if !server.format_enabled() {
        return Ok(None);
    }

    let uri = params.text_document.uri.as_str();

    if let Some(snapshot) = server.analysis_map.get(uri)
        && !snapshot.has_parse_errors()
        && let Some(ast) = snapshot.ast()
        && let Some(doc) = server.documents.get(uri)
    {
        Ok(format_program(ast, &doc.rope))
    } else {
        Ok(None)
    }
}

pub fn format_program(ast: &IDLMergedProg, rope: &Rope) -> Option<Vec<TextEdit>> {
    let original_text = rope.to_string();
    let (imports, service_name) = extract_imports_and_service_name(&original_text);
    let mut formatted_text = pretty_print(ast);

    if let Some(name) = service_name {
        inject_service_name(&mut formatted_text, &name);
    }

    if !imports.is_empty() && !formatted_has_imports(&formatted_text) {
        formatted_text = prepend_imports(&formatted_text, &imports);
    }

    let last_line_idx = rope.len_lines().saturating_sub(1);
    let last_line = rope.line(last_line_idx);
    let last_char_col = last_line.len_chars();
    let end_position = Position::new(last_line_idx as u32, last_char_col as u32);

    let start_position = Position::new(0, 0);
    let full_range = Range::new(start_position, end_position);

    Some(vec![TextEdit {
        range: full_range,
        new_text: formatted_text,
    }])
}

fn extract_imports_and_service_name(src: &str) -> (Vec<String>, Option<String>) {
    (extract_imports(src), extract_service_name(src))
}

fn extract_imports(src: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut lines = src.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("import") {
            continue;
        }

        let mut statement = String::from(line);
        if !line.trim_end().ends_with(';') {
            for next in &mut lines {
                statement.push('\n');
                statement.push_str(next);
                if next.trim_end().ends_with(';') {
                    break;
                }
            }
        }

        imports.push(statement);
    }

    imports
}

fn extract_service_name(src: &str) -> Option<String> {
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("service") {
            continue;
        }

        let rest = trimmed["service".len()..].trim_start();
        let mut chars = rest.char_indices();
        let (start_idx, first) = chars.next()?;
        if !is_ident_start(first) {
            continue;
        }

        let mut end_idx = start_idx + first.len_utf8();
        for (idx, ch) in chars {
            if !is_ident_continue(ch) {
                end_idx = idx;
                break;
            }
            end_idx = idx + ch.len_utf8();
        }

        let name = &rest[start_idx..end_idx];
        let after = rest[end_idx..].trim_start();
        if after.starts_with(':') {
            return Some(name.to_string());
        }
    }

    None
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn inject_service_name(formatted: &mut String, name: &str) -> bool {
    let mut search_from = 0;
    while let Some(idx) = formatted[search_from..].find("service") {
        let start = search_from + idx;
        if start > 0
            && let Some(prev) = formatted[..start].chars().last()
            && is_ident_continue(prev)
        {
            search_from = start + 1;
            continue;
        }

        let cursor = start + "service".len();
        let mut colon_idx = None;
        for (offset, ch) in formatted[cursor..].char_indices() {
            if ch.is_whitespace() {
                continue;
            }
            if ch == ':' {
                colon_idx = Some(cursor + offset);
            }
            break;
        }

        if let Some(colon_pos) = colon_idx {
            formatted.replace_range(start..(colon_pos + 1), &format!("service {} :", name));
            return true;
        }

        search_from = start + 1;
    }

    false
}

fn formatted_has_imports(formatted: &str) -> bool {
    formatted
        .lines()
        .any(|line| line.trim_start().starts_with("import"))
}

fn prepend_imports(formatted: &str, imports: &[String]) -> String {
    let mut output = String::new();
    output.push_str(&imports.join("\n"));
    output.push('\n');
    if !formatted.starts_with('\n') {
        output.push('\n');
    }
    output.push_str(formatted.trim_start_matches('\n'));
    output
}
