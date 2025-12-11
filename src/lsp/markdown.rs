use std::fmt::Write;

/// Incrementally builds Markdown sections that can be shared between hover
/// responses and completion item documentation.
#[derive(Default)]
pub struct MarkdownWriter {
    buffer: String,
    sections: usize,
}

impl MarkdownWriter {
    pub fn is_empty(&self) -> bool {
        self.sections == 0
    }

    pub fn push_text(&mut self, text: impl AsRef<str>) {
        self.start_section();
        self.buffer.push_str(text.as_ref());
    }

    pub fn push_rule(&mut self) {
        self.start_section();
        self.buffer.push_str("---");
    }

    pub fn push_code_block(&mut self, snippet: &str) {
        self.start_section();
        let _ = writeln!(self.buffer, "```candid");
        self.buffer.push_str(snippet);
        if !snippet.ends_with('\n') {
            self.buffer.push('\n');
        }
        let _ = write!(self.buffer, "```");
    }

    pub fn finish(self) -> Option<String> {
        if self.sections == 0 {
            None
        } else {
            Some(self.buffer)
        }
    }

    fn start_section(&mut self) {
        if self.sections > 0 {
            self.buffer.push_str("\n\n");
        }
        self.sections += 1;
    }
}

pub fn push_snippet_with_docs(writer: &mut MarkdownWriter, snippet: &str, docs: Option<&str>) {
    writer.push_code_block(snippet);
    push_docs_section(writer, docs);
}

pub fn push_docs_section(writer: &mut MarkdownWriter, docs: Option<&str>) {
    if let Some(doc) = docs {
        writer.push_rule();
        writer.push_text(doc);
    }
}

pub fn snippet_with_docs_markdown(snippet: &str, docs: Option<&str>) -> Option<String> {
    let mut writer = MarkdownWriter::default();
    push_snippet_with_docs(&mut writer, snippet, docs);
    writer.finish()
}

pub fn text_markdown(text: &str) -> Option<String> {
    let mut writer = MarkdownWriter::default();
    writer.push_text(text);
    writer.finish()
}
