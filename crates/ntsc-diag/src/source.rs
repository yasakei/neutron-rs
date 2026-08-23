//! Source text buffer with O(1) line-number lookup.
//!
//! Given a byte offset, reports the line number, column, and full text of the
//! enclosing line; built once per file and reused across many diagnostics.

#[derive(Debug, Clone)]
pub struct SourceBuffer {
    text: String,

    /// Byte offset of the *start* of each line: `line_starts[i]` is the first
    /// byte of line `i+1`, with a trailing sentinel of `text.len()`.
    line_starts: Vec<usize>,

    file_path: String,
}

impl SourceBuffer {
    pub fn new(text: &str, file_path: impl Into<String>) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                text.match_indices('\n')
                    .map(|(i, _)| i + 1)
                    .filter(|&i| i > 0),
            )
            .chain(std::iter::once(text.len()))
            .collect();

        Self {
            text: text.to_string(),
            line_starts,
            file_path: file_path.into(),
        }
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len().saturating_sub(1)
    }

    /// 1-based line number for a byte offset; returns 0 when `offset` is out
    /// of range.
    pub fn line_at_offset(&self, offset: usize) -> usize {
        if offset > self.text.len() {
            return 0;
        }

        match self.line_starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }

    pub fn col_at_offset(&self, offset: usize) -> usize {
        if self.line_starts.is_empty() {
            return 0;
        }
        let line = self.line_at_offset(offset);
        if line == 0 {
            return 0;
        }
        let line_start = self.line_starts[line.saturating_sub(1)];
        offset.saturating_sub(line_start) + 1
    }

    /// Text of line (1-based) without its trailing newline; `""` for
    /// out-of-range lines.
    pub fn line_text(&self, line: usize) -> &str {
        if line == 0 || line >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());

        let slice = &self.text[start..end];
        // Strip trailing `\r` (Windows) and `\n` line endings.
        let slice = slice.strip_suffix('\r').unwrap_or(slice);
        slice.strip_suffix('\n').unwrap_or(slice)
    }

    /// Line-number gutter prefix with padding, e.g. `" 3 │ "`.
    pub fn line_number_prefix(line: usize, digits: usize) -> String {
        format!("{:>digits$} │ ", line, digits = digits)
    }
}

#[derive(Debug, Clone, Default)]
/// Source files keyed by path; the writer resolves buffers here so
/// diagnostics can render snippets across multiple files.
pub struct SourceMap {
    files: Vec<SourceBuffer>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Add a buffer; a later buffer with the same path replaces the earlier one.
    pub fn add(&mut self, buffer: SourceBuffer) {
        let path = buffer.file_path().to_string();
        self.files.retain(|f| f.file_path() != path);
        self.files.push(buffer);
    }

    pub fn get(&self, path: &str) -> Option<&SourceBuffer> {
        self.files.iter().find(|f| f.file_path() == path)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_lines() {
        let src = SourceBuffer::new("a\nbc\ndef\n", "test.nt");

        assert_eq!(src.line_count(), 4);
        assert_eq!(src.line_text(1), "a");
        assert_eq!(src.line_text(2), "bc");
        assert_eq!(src.line_text(3), "def");
        assert_eq!(src.line_text(4), "");
    }

    #[test]
    fn offset_to_line_column() {
        let src = SourceBuffer::new("hello\nworld\n", "test.nt");

        assert_eq!(src.line_at_offset(0), 1);
        assert_eq!(src.col_at_offset(0), 1);

        assert_eq!(src.line_at_offset(6), 2);
        assert_eq!(src.col_at_offset(6), 1);

        assert_eq!(src.line_at_offset(7), 2);
        assert_eq!(src.col_at_offset(7), 2);
    }

    #[test]
    fn trailing_newline() {
        let src = SourceBuffer::new("line1\nline2\n", "test.nt");

        assert_eq!(src.line_count(), 3);
        assert_eq!(src.line_text(2), "line2");
        assert_eq!(src.line_text(3), "");
    }

    #[test]
    fn no_trailing_newline() {
        let src = SourceBuffer::new("line1\nline2", "test.nt");
        assert_eq!(src.line_count(), 2);
        assert_eq!(src.line_text(2), "line2");
    }

    #[test]
    fn source_map_lookup_by_path() {
        let mut map = SourceMap::new();
        map.add(SourceBuffer::new("fun a() {}\n", "a.nt"));
        map.add(SourceBuffer::new("fun b() {}\n", "b.nt"));

        assert_eq!(map.len(), 2);
        assert!(map.get("a.nt").is_some());
        assert!(map.get("b.nt").is_some());
        assert!(map.get("c.nt").is_none());

        map.add(SourceBuffer::new("fun a2() {}\n", "a.nt"));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a.nt").unwrap().text(), "fun a2() {}\n");
    }
}
