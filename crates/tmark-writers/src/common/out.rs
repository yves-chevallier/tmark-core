//! The output buffer: a string plus the continuation-line prefixes of the
//! enclosing containers, and the source-map entries opened by `begin` and
//! closed by `end` (design 07 §Source maps; writers-and-passes.md §1:
//! "`tmark-fmt::out::Out` plus `begin(NodeId)`/`end(NodeId)`").

use std::ops::Range;

use tmark_ir::NodeId;

use crate::SourceMap;

/// A writer's output buffer.
#[derive(Debug, Default)]
pub struct Out {
    buf: String,
    prefixes: Vec<String>,
    /// Text has been written on the current line (prefixes included).
    line_started: bool,
    /// Record `begin`/`end` pairs; off leaves the map empty.
    mapping: bool,
    open: Vec<(NodeId, u32)>,
    entries: Vec<(Range<u32>, NodeId)>,
}

impl Out {
    pub fn new(mapping: bool) -> Self {
        Out {
            mapping,
            ..Default::default()
        }
    }

    /// A buffer for a fragment rendered aside (a caption, a title): no map.
    pub fn scratch() -> Self {
        Out::default()
    }

    /// Writes `text`. A `\n` ends the line; the next character that is not
    /// a newline first writes the prefixes.
    pub fn push(&mut self, text: &str) {
        for ch in text.chars() {
            self.push_char(ch);
        }
    }

    pub fn push_char(&mut self, ch: char) {
        if ch == '\n' {
            if !self.line_started {
                self.write_prefixes(true);
            }
            self.buf.push('\n');
            self.line_started = false;
        } else {
            if !self.line_started {
                self.write_prefixes(false);
                self.line_started = true;
            }
            self.buf.push(ch);
        }
    }

    fn write_prefixes(&mut self, trimmed: bool) {
        let joined: String = self.prefixes.concat();
        if trimmed {
            self.buf.push_str(joined.trim_end());
        } else {
            self.buf.push_str(&joined);
        }
    }

    /// Ends the current line if it carries text.
    pub fn ensure_newline(&mut self) {
        if self.line_started {
            self.push_char('\n');
        }
    }

    /// Ends the current line if needed, then makes sure one empty line
    /// separates what follows from what precedes (never two).
    pub fn blank_line(&mut self) {
        self.ensure_newline();
        if !self.buf.is_empty() && !self.buf.ends_with("\n\n") {
            self.push_char('\n');
        }
    }

    /// Adds a prefix for the lines that follow the current one.
    pub fn push_prefix(&mut self, prefix: &str) {
        self.prefixes.push(prefix.to_string());
    }

    pub fn pop_prefix(&mut self) {
        self.prefixes.pop();
    }

    /// Nothing has been written on the current line yet.
    pub fn at_line_start(&self) -> bool {
        !self.line_started
    }

    /// The last character written, prefixes included.
    pub fn last_char(&self) -> Option<char> {
        self.buf.chars().next_back()
    }

    /// What has been written ends with `suffix`, trailing whitespace and
    /// line ends ignored.
    pub fn ends_with(&self, suffix: &str) -> bool {
        self.buf.trim_end().ends_with(suffix)
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Bytes written so far.
    pub fn len(&self) -> u32 {
        self.buf.len() as u32
    }

    /// Removes trailing spaces and tabs on the current line.
    pub fn trim_trailing_spaces(&mut self) {
        let trimmed = self.buf.trim_end_matches([' ', '\t']).len();
        self.buf.truncate(trimmed);
    }

    /// Opens a source-map entry for `id` at the current offset.
    pub fn begin(&mut self, id: NodeId) {
        if self.mapping {
            self.open.push((id, self.len()));
        }
    }

    /// Closes the entry of `id`; unbalanced calls are ignored.
    pub fn end(&mut self, id: NodeId) {
        if !self.mapping {
            return;
        }
        if let Some(pos) = self.open.iter().rposition(|(open, _)| *open == id) {
            let (_, start) = self.open.remove(pos);
            let end = self.len();
            if end > start {
                self.entries.push((start..end, id));
            }
        }
    }

    /// The text, ending with exactly one newline (empty when nothing was
    /// written), and the source map in document order.
    pub fn finish(mut self) -> (String, SourceMap) {
        let trimmed = self.buf.trim_end_matches('\n').len();
        self.buf.truncate(trimmed);
        if !self.buf.is_empty() {
            self.buf.push('\n');
        }
        let len = self.buf.len() as u32;
        let mut entries = self.entries;
        for (range, _) in entries.iter_mut() {
            range.end = range.end.min(len);
        }
        entries.retain(|(range, _)| range.end > range.start);
        entries.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.end)));
        (self.buf, SourceMap { entries })
    }

    /// The text alone, trailing newlines trimmed: for fragments rendered
    /// aside.
    pub fn finish_text(self) -> String {
        let (text, _) = self.finish();
        text.trim_end_matches('\n').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_apply_from_the_next_line() {
        let mut out = Out::new(false);
        out.push("- ");
        out.push_prefix("  ");
        out.push("first\nsecond\n");
        out.blank_line();
        out.push("third\n");
        out.pop_prefix();
        out.push("- next\n");
        assert_eq!(out.finish().0, "- first\n  second\n\n  third\n- next\n");
    }

    #[test]
    fn blank_line_never_doubles() {
        let mut out = Out::new(false);
        out.push("a\n");
        out.blank_line();
        out.blank_line();
        out.push("b");
        assert_eq!(out.finish().0, "a\n\nb\n");
    }

    #[test]
    fn map_entries_cover_what_was_written() {
        let mut out = Out::new(true);
        out.begin(NodeId(1));
        out.push("abc\n");
        out.begin(NodeId(2));
        out.push("de");
        out.end(NodeId(2));
        out.push("\n");
        out.end(NodeId(1));
        let (text, map) = out.finish();
        assert_eq!(text, "abc\nde\n");
        assert_eq!(map.entries, vec![(0..7, NodeId(1)), (4..6, NodeId(2))]);
        assert_eq!(map.node_at(5), Some(NodeId(2)));
        assert_eq!(map.node_at(1), Some(NodeId(1)));
        assert_eq!(map.node_at(9), None);
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(json, "[[0,7,1],[4,6,2]]");
        let back: SourceMap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, map);
    }

    #[test]
    fn no_map_when_off() {
        let mut out = Out::new(false);
        out.begin(NodeId(1));
        out.push("x");
        out.end(NodeId(1));
        assert!(out.finish().1.entries.is_empty());
    }
}
