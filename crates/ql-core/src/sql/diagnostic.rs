use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub content: String,
    pub line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        let mut line_starts = vec![0];

        for (index, byte) in content.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }

        Self {
            name: name.into(),
            content,
            line_starts,
        }
    }

    pub fn line_text(&self, line: usize) -> Option<&str> {
        let start = *self.line_starts.get(line.checked_sub(1)?)?;
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.content.len());

        let slice = &self.content[start..end.min(self.content.len())];
        let slice = slice.strip_suffix('\n').unwrap_or(slice);

        Some(slice.strip_suffix('\r').unwrap_or(slice))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file_id: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(file_id: usize, start: usize, end: usize) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    pub fn point(file_id: usize, offset: usize) -> Self {
        Self {
            file_id,
            start: offset,
            end: offset,
        }
    }

    pub fn line_col(&self, file: &SourceFile) -> (usize, usize) {
        let offset = self.start.min(file.content.len());

        let line_index = file
            .line_starts
            .partition_point(|line_start| *line_start <= offset)
            .saturating_sub(1);

        let line_start = file.line_starts[line_index];

        (line_index + 1, offset.saturating_sub(line_start) + 1)
    }

    pub fn snippet(&self, file: &SourceFile, context_lines: usize) -> String {
        let (line, _) = self.line_col(file);
        let start_line = line.saturating_sub(context_lines).max(1);
        let end_line = (line + context_lines).min(file.line_starts.len());

        let width = end_line.to_string().len();
        let mut rendered = String::new();

        for current_line in start_line..=end_line {
            if let Some(text) = file.line_text(current_line) {
                let _ = writeln!(rendered, "{current_line:>width$} | {text}", width = width);
            }
        }

        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn render(&self, files: &[SourceFile]) -> String {
        let mut rendered = String::new();

        let severity = match self.severity {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Note => "Note",
            Severity::Help => "Help",
        };

        match &self.code {
            Some(code) => {
                let _ = writeln!(rendered, "{severity}[{code}]: {}", self.message);
            }
            None => {
                let _ = writeln!(rendered, "{severity}: {}", self.message);
            }
        }

        for label in &self.labels {
            let Some(file) = files.get(label.span.file_id) else {
                let _ = writeln!(rendered, "  --> <unknown>:{}:{}", label.span.start + 1, 1);
                continue;
            };

            let (line, column) = label.span.line_col(file);
            let line_text = file.line_text(line).unwrap_or("");

            let underline_width = label.span.end.saturating_sub(label.span.start).max(1);

            // One gutter width used everywhere.
            let gutter_width = line.to_string().len();

            let _ = writeln!(
                rendered,
                "{:>width$} --> {}:{}:{}",
                "",
                file.name,
                line,
                column,
                width = gutter_width,
            );

            let _ = writeln!(rendered, "{:>width$} |", "", width = gutter_width,);

            let _ = writeln!(
                rendered,
                "{line:>width$} | {line_text}",
                width = gutter_width,
            );

            let mut marker = String::new();

            marker.push_str(&" ".repeat(column.saturating_sub(1)));
            marker.push('^');

            if underline_width > 1 {
                marker.push_str(&"~".repeat(underline_width - 1));
            }

            if !label.message.is_empty() {
                marker.push(' ');
                marker.push_str(&label.message);
            }

            let _ = writeln!(rendered, "{:>width$} | {marker}", "", width = gutter_width,);
        }

        for note in &self.notes {
            let _ = writeln!(rendered, "   = note: {note}");
        }

        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_line_and_column() {
        let file = SourceFile::new("query.sql", "SELECT\nname");
        let span = Span::new(0, 8, 12);

        assert_eq!(span.line_col(&file), (2, 2));
    }

    #[test]
    fn renders_single_label_diagnostic() {
        let file = SourceFile::new("query.sql", "SELECT name functions");

        let diagnostic = Diagnostic {
            severity: Severity::Error,
            code: Some("E001".to_string()),
            message: "expected FROM".to_string(),
            labels: vec![Label {
                span: Span::new(0, 12, 21),
                message: String::new(),
            }],
            notes: vec!["try adding a FROM clause".to_string()],
        };

        let rendered = diagnostic.render(&[file]);

        assert!(rendered.contains("Error[E001]: expected FROM"));
        assert!(rendered.contains("query.sql:1:13"));
        assert!(rendered.contains("^~~~~~~~~"));
        assert!(rendered.contains("note: try adding a FROM clause"));
    }
}
