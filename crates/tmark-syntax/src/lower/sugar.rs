//! Inline sugar found inside text runs (spec Appendix "PyMdownX
//! compatibility profile"): emoji shortcodes `:smile:` expanded from the
//! `gemoji` table, the Material icon shortcodes `:material-cog:` (§Emoji
//! and icon shortcodes), and the reference-style link `[text][id]`
//! (§Ref). The progress bar, the smart symbols and the straight double
//! quotes (§Quoted) live in `tmark_ir::sugar`, shared with the printer
//! that escapes them; `inline.rs` turns the pieces into nodes.

use std::ops::Range;

use tmark_ir::emoji;

pub use tmark_ir::sugar::{progress_bar, quoted, smart_symbol, Progress};

/// A shortcode at `at` in `text` (a `:` there): its byte length and what
/// it is. A colon-delimited word touching a word character on either side
/// (`12:30:45`, `a:b:c`) is not one; a name the emoji table does not know
/// is not one either, so it stays literal with no diagnostic.
pub enum Shortcode {
    Emoji(&'static str),
    Icon,
}

pub fn shortcode(text: &str, at: usize) -> Option<(usize, Shortcode)> {
    let rest = &text[at..];
    if !rest.starts_with(':') || text[..at].ends_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let body = &rest[1..];
    let len = body
        .bytes()
        .take_while(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'+')
        })
        .count();
    if len == 0 || !body[len..].starts_with(':') {
        return None;
    }
    if body[len + 1..].starts_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let name = &body[..len];
    let kind = if emoji::is_icon(name) {
        Shortcode::Icon
    } else {
        Shortcode::Emoji(emoji::emoji(name)?)
    };
    Some((len + 2, kind))
}

/// A reference-style link `[text][id]` (spec §Ref, "reference-style
/// form"): where its text sits in the run, the id it names, and the byte
/// length of the whole spelling.
#[derive(Debug, PartialEq)]
pub struct Reference {
    pub len: usize,
    pub text: Range<usize>,
    pub id: String,
}

/// Whether `s` is the spec's `id` production (§Identifiers):
/// `prefix(:key)+` or a bare `key`.
fn is_id(s: &str) -> bool {
    let mut segments = s.split(':');
    let head = segments.next().unwrap_or_default();
    let key = |k: &str| {
        let mut bytes = k.bytes();
        bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
            && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    let mut rest = segments.peekable();
    if rest.peek().is_none() {
        return key(head);
    }
    let prefix = {
        let mut bytes = head.bytes();
        bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
            && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    };
    prefix && rest.all(key)
}

/// The closing half of the spelling, `][id]` at the start of `s`: the id
/// it names and the byte length of the half. The id is an identifier, so
/// `[a][b c]` is not a reference and neither is the wiki link `[[page]]`;
/// the collapsed form `[text][]` names no id and is not one either.
pub fn reference_tail(s: &str) -> Option<(String, usize)> {
    let rest = s.strip_prefix("][")?;
    let end = rest.find(']')?;
    let id = &rest[..end];
    is_id(id).then(|| (id.to_string(), end + 3))
}

/// A reference-style link at `at` in `text` (a `[` there), the text of
/// which is one run. Only the form CommonMark leaves undefined reaches
/// this scan — a defined one is a link the tokenizer built. A text
/// carrying markup is not one text node, and is cut out of the
/// neighbouring nodes instead (`inline.rs`, `Lowerer::reference_cut`).
/// The text is not empty and does not start the footnote spelling
/// `[^…]`; and a `!` before the bracket is an image reference, which is
/// not a reference to a label.
pub fn reference_link(text: &str, at: usize) -> Option<Reference> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'[') || (at > 0 && bytes[at - 1] == b'!') {
        return None;
    }
    let start = at + 1;
    let mut depth = 0usize;
    let mut end = None;
    for (i, byte) in text[start..].bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' if depth == 0 => {
                end = Some(start + i);
                break;
            }
            b']' => depth -= 1,
            b'\n' => return None,
            _ => {}
        }
    }
    let end = end?;
    if end == start || bytes[start] == b'^' {
        return None;
    }
    let (id, tail) = reference_tail(&text[end..])?;
    Some(Reference {
        len: end + tail - at,
        text: start..end,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcodes() {
        assert!(matches!(
            shortcode(":smile: x", 0),
            Some((7, Shortcode::Emoji("\u{1f604}")))
        ));
        assert!(matches!(
            shortcode("a :+1:", 2),
            Some((4, Shortcode::Emoji(_)))
        ));
        assert!(matches!(
            shortcode(":material-cog:", 0),
            Some((14, Shortcode::Icon))
        ));
        assert!(shortcode("12:30:45", 2).is_none());
        assert!(shortcode("a:b:c", 1).is_none());
        assert!(shortcode(":nope-not-an-emoji:", 0).is_none());
        assert!(shortcode(":Smile:", 0).is_none());
        assert!(shortcode("note: text:", 4).is_none());
    }

    #[test]
    fn reference_style_links() {
        assert_eq!(
            reference_link("[Plus haut][opengl-coordinates], voir", 0),
            Some(Reference {
                len: 31,
                text: 1..10,
                id: "opengl-coordinates".to_string(),
            })
        );
        assert_eq!(
            reference_link("see [the trace][fig:trace] there", 4),
            Some(Reference {
                len: 22,
                text: 5..14,
                id: "fig:trace".to_string(),
            })
        );
        // Brackets inside the text nest.
        assert_eq!(
            reference_link("[a [b] c][id]", 0).map(|r| r.id),
            Some("id".to_string())
        );
        // Not references: a wiki link, a footnote, an empty text, a
        // collapsed or shortcut form, an id that is not an identifier, a
        // text running over a line.
        assert!(reference_link("[[page]] and", 0).is_none());
        assert!(reference_link("[[page]] and", 1).is_none());
        assert!(reference_link("[^note][id]", 0).is_none());
        assert!(reference_link("[][id]", 0).is_none());
        assert!(reference_link("[text][]", 0).is_none());
        assert!(reference_link("[text] and", 0).is_none());
        assert!(reference_link("[text][an id]", 0).is_none());
        assert!(reference_link("[text][-id]", 0).is_none());
        assert!(reference_link("[a\nb][id]", 0).is_none());
        assert!(reference_link("![alt][id]", 1).is_none());
    }
}
