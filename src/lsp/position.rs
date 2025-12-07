use ropey::Rope;
use tower_lsp_server::ls_types::{Position, Range};

use crate::lsp::span::Span;

/// Convert a character-offset into an LSP `Position`.
pub fn offset_to_position(offset: usize, rope: &Rope) -> Option<Position> {
    let line = rope.try_char_to_line(offset).ok()?;
    let first_char_of_line = rope.try_line_to_char(line).ok()?;
    let column = offset.saturating_sub(first_char_of_line);
    Some(Position::new(line as u32, column as u32))
}

/// Convert an LSP `Position` into a character-offset.
pub fn position_to_offset(position: Position, rope: &Rope) -> Option<usize> {
    let line_idx = position.line as usize;
    if line_idx >= rope.len_lines() {
        return None;
    }

    let line_start = rope.try_line_to_char(line_idx).ok()?;
    let column = position.character as usize;
    let line_slice = rope.line(line_idx);
    if column > line_slice.len_chars() {
        return None;
    }

    Some(line_start + column)
}

/// Convert a semantic `Span` into an LSP `Range`.
pub fn span_to_range(span: &Span, rope: &Rope) -> Option<Range> {
    let start = offset_to_position(span.start, rope)?;
    let end = offset_to_position(span.end, rope)?;
    Some(Range::new(start, end))
}
