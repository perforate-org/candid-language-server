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
        let options = FormatOptions {
            indent_width: server.format_indent_width(),
            blank_lines: server.format_blank_lines(),
        };
        Ok(format_program_with_options(ast, &doc.rope, &options))
    } else {
        Ok(None)
    }
}

pub fn format_program(ast: &IDLMergedProg, rope: &Rope) -> Option<Vec<TextEdit>> {
    format_program_with_options(ast, rope, &FormatOptions::default())
}

#[derive(Debug, Default, Clone)]
pub struct FormatOptions {
    pub indent_width: Option<usize>,
    pub blank_lines: Option<usize>,
}

pub fn format_program_with_options(
    ast: &IDLMergedProg,
    rope: &Rope,
    options: &FormatOptions,
) -> Option<Vec<TextEdit>> {
    let original_text = rope.to_string();
    let (imports, service_name) = extract_imports_and_service_name(&original_text);
    let orphan_comments = extract_orphan_comment_blocks(&original_text);
    let mut formatted_text = pretty_print(ast);

    if let Some(name) = service_name {
        inject_service_name(&mut formatted_text, &name);
    }

    if !imports.is_empty() && !formatted_has_imports(&formatted_text) {
        formatted_text = prepend_imports(&formatted_text, &imports);
    }
    if !orphan_comments.is_empty() {
        formatted_text = inject_orphan_comment_blocks(&formatted_text, &orphan_comments);
    }
    if let Some(width) = options.indent_width {
        formatted_text = apply_indent_width(&formatted_text, width);
    }
    if let Some(lines) = options.blank_lines {
        formatted_text = collapse_blank_lines(&formatted_text, lines);
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
    let mut imports = Vec::new();
    let mut service_name = None;
    let mut lines = src.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import") {
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
            continue;
        }

        if service_name.is_none() {
            service_name = extract_service_name_from_line(trimmed);
        }
    }

    (imports, service_name)
}

fn extract_service_name_from_line(trimmed: &str) -> Option<String> {
    if !trimmed.starts_with("service") {
        return None;
    }

    let rest = trimmed["service".len()..].trim_start();
    let mut chars = rest.char_indices();
    let (start_idx, first) = chars.next()?;
    if !is_ident_start(first) {
        return None;
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

#[derive(Debug, Clone)]
struct OrphanCommentBlock {
    text: String,
    anchor_next: Option<String>,
    anchor_next_occurrence: usize,
    anchor_prev: Option<String>,
    anchor_prev_occurrence: usize,
    before_first: bool,
}

fn extract_orphan_comment_blocks(src: &str) -> Vec<OrphanCommentBlock> {
    let lines: Vec<&str> = src.lines().collect();
    let mut normalized: Vec<Option<String>> = Vec::with_capacity(lines.len());
    let mut line_occurrence = vec![0usize; lines.len()];
    let mut seen_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            normalized.push(None);
            continue;
        }
        let norm = normalize_line(trimmed);
        let count = seen_counts
            .entry(norm.clone())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        line_occurrence[idx] = *count;
        normalized.push(Some(norm));
    }

    let mut blocks = Vec::new();
    let mut idx = 0usize;
    let mut last_non_comment_idx: Option<usize> = None;
    while idx < lines.len() {
        let line = lines[idx];
        if !is_comment_line(line) {
            if !line.trim().is_empty() {
                last_non_comment_idx = Some(idx);
            }
            idx += 1;
            continue;
        }

        let start = idx;
        let mut end = idx;
        while end + 1 < lines.len() && is_comment_line(lines[end + 1]) {
            end += 1;
        }

        let next_idx = end + 1;
        if next_idx < lines.len() && is_import_line(lines[next_idx]) {
            let anchor_next_idx = Some(next_idx);
            let anchor_next = anchor_next_idx.and_then(|i| normalized[i].as_ref().cloned());
            let anchor_next_occurrence = anchor_next_idx.map(|i| line_occurrence[i]).unwrap_or(0);

            let before_first = last_non_comment_idx.is_none();
            let anchor_prev = last_non_comment_idx.and_then(|i| normalized[i].as_ref().cloned());
            let anchor_prev_occurrence = last_non_comment_idx
                .map(|i| line_occurrence[i])
                .unwrap_or(0);

            let text = lines[start..=end].join("\n");
            blocks.push(OrphanCommentBlock {
                text,
                anchor_next,
                anchor_next_occurrence,
                anchor_prev,
                anchor_prev_occurrence,
                before_first,
            });
        } else if next_idx < lines.len() && lines[next_idx].trim().is_empty() {
            let anchor_next_idx = (next_idx + 1..lines.len()).find(|i| {
                let trimmed = lines[*i].trim();
                !trimmed.is_empty() && !trimmed.starts_with("//")
            });
            let anchor_next = anchor_next_idx.and_then(|i| normalized[i].as_ref().cloned());
            let anchor_next_occurrence = anchor_next_idx.map(|i| line_occurrence[i]).unwrap_or(0);

            let before_first = last_non_comment_idx.is_none();
            let anchor_prev = last_non_comment_idx.and_then(|i| normalized[i].as_ref().cloned());
            let anchor_prev_occurrence = last_non_comment_idx
                .map(|i| line_occurrence[i])
                .unwrap_or(0);

            let text = lines[start..=end].join("\n");
            blocks.push(OrphanCommentBlock {
                text,
                anchor_next,
                anchor_next_occurrence,
                anchor_prev,
                anchor_prev_occurrence,
                before_first,
            });
        } else if let Some(last) = last_non_comment_idx
            && !lines[last].trim().is_empty()
        {
            last_non_comment_idx = Some(last);
        }

        idx = end + 1;
    }

    blocks
}

fn inject_orphan_comment_blocks(formatted: &str, blocks: &[OrphanCommentBlock]) -> String {
    if blocks.is_empty() {
        return formatted.to_string();
    }

    let mut lines: Vec<std::borrow::Cow<'_, str>> =
        formatted.lines().map(std::borrow::Cow::Borrowed).collect();
    for block in blocks {
        if formatted.contains(&block.text) {
            continue;
        }

        let mut inserted = false;
        if let Some(anchor) = block.anchor_next.as_ref()
            && let Some(insert_at) = find_anchor_index(&lines, anchor, block.anchor_next_occurrence)
        {
            insert_block_before(&mut lines, insert_at, block);
            inserted = true;
        }

        if !inserted
            && block.before_first
            && let Some(insert_at) = first_content_line_index(&lines)
        {
            insert_block_before(&mut lines, insert_at, block);
            inserted = true;
        }

        if !inserted
            && let Some(anchor) = block.anchor_prev.as_ref()
            && let Some(insert_at) = find_anchor_index(&lines, anchor, block.anchor_prev_occurrence)
        {
            let after = insert_at + 1;
            insert_block_after(&mut lines, after, block);
            inserted = true;
        }

        if !inserted {
            if !lines.is_empty() && !lines.last().unwrap().as_ref().trim().is_empty() {
                lines.push(std::borrow::Cow::Borrowed(""));
            }
            lines.extend(
                block
                    .text
                    .lines()
                    .map(|line| std::borrow::Cow::Owned(line.to_string())),
            );
            lines.push(std::borrow::Cow::Borrowed(""));
        }
    }

    let mut output = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        output.push_str(line.as_ref());
    }
    output
}

fn first_content_line_index(lines: &[std::borrow::Cow<'_, str>]) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.as_ref().trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        return Some(idx);
    }
    None
}

fn find_anchor_index(
    lines: &[std::borrow::Cow<'_, str>],
    anchor: &str,
    occurrence: usize,
) -> Option<usize> {
    if occurrence == 0 {
        return None;
    }
    let mut count = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.as_ref().trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if normalize_line(trimmed) == *anchor {
            count += 1;
            if count == occurrence {
                return Some(idx);
            }
        }
    }
    None
}

fn insert_block_before(
    lines: &mut Vec<std::borrow::Cow<'_, str>>,
    insert_at: usize,
    block: &OrphanCommentBlock,
) {
    let mut chunk: Vec<std::borrow::Cow<'_, str>> = block
        .text
        .lines()
        .map(|line| std::borrow::Cow::Owned(line.to_string()))
        .collect();
    if insert_at < lines.len() && !lines[insert_at].as_ref().trim().is_empty() {
        chunk.push(std::borrow::Cow::Borrowed(""));
    }
    lines.splice(insert_at..insert_at, chunk);
}

fn insert_block_after(
    lines: &mut Vec<std::borrow::Cow<'_, str>>,
    insert_at: usize,
    block: &OrphanCommentBlock,
) {
    let mut chunk: Vec<std::borrow::Cow<'_, str>> = Vec::new();
    if insert_at > 0 && !lines[insert_at - 1].as_ref().trim().is_empty() {
        chunk.push(std::borrow::Cow::Borrowed(""));
    }
    chunk.extend(
        block
            .text
            .lines()
            .map(|line| std::borrow::Cow::Owned(line.to_string())),
    );
    chunk.push(std::borrow::Cow::Borrowed(""));
    lines.splice(insert_at..insert_at, chunk);
}

fn is_comment_line(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn is_import_line(line: &str) -> bool {
    line.trim_start().starts_with("import")
}

fn normalize_line(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn apply_indent_width(formatted: &str, indent_width: usize) -> String {
    if indent_width == 0 {
        return formatted.to_string();
    }

    const BASE_INDENT_WIDTH: usize = 2;
    let mut output = String::with_capacity(formatted.len());
    for (idx, line) in formatted.lines().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        if line.trim().is_empty() {
            continue;
        }

        let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
        let remainder = &line[leading_spaces..];
        if leading_spaces % BASE_INDENT_WIDTH != 0 {
            output.push_str(line);
            continue;
        }
        let level = leading_spaces / BASE_INDENT_WIDTH;
        output.extend(std::iter::repeat_n(' ', level * indent_width));
        output.push_str(remainder);
    }
    if formatted.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn collapse_blank_lines(formatted: &str, max_blank_lines: usize) -> String {
    let mut lines = Vec::new();
    let mut blank_run = 0usize;
    for line in formatted.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= max_blank_lines {
                lines.push(line.to_string());
            }
            continue;
        }
        blank_run = 0;
        lines.push(line.to_string());
    }
    let mut output = lines.join("\n");
    if formatted.ends_with('\n') {
        output.push('\n');
    }
    output
}
