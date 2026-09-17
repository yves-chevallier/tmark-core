//! Typst markup escaping, from TeXSmith's `writers/typst/escaper.py`: the
//! characters that carry meaning in markup mode are backslash-escaped.
//!
//! `_ESCAPE_CHARS` covers only the characters that are always special
//! (`\ # $ * _ ` < > @ [ ]`); TeXSmith's Python writer never emitted plain
//! `Str` runs that could start a line, so it never needed the rest. TMark
//! does, so the rest are context-sensitive (verified against typst
//! 0.15.1 and 0.14.2, see `crates/tmark-writers/src/typst/escape.rs` tests and
//! `design/07-writers.md` §Implementation notes):
//!
//! - `~` is always a non-breaking space in markup, so it is escaped
//!   unconditionally, like LaTeX's `\textasciitilde{}`.
//! - `/` opens a line comment (`//`) or a block comment (`/*`) *anywhere*
//!   in markup, not just at a line start — an unescaped `//` silently eats
//!   the rest of the line. Escaping the first slash of the pair is enough:
//!   `\//` and `\/*` both read back as literal text, because the escape
//!   consumes exactly one source character and the parser re-checks from
//!   the next one, which is no longer paired.
//! - `=`, `+`, `-` and `/` are also structural at the *start of a line*:
//!   Typst's grammar considers "line start" to mean the start of the
//!   document, right after a soft or hard line break, or right after any
//!   markup content block opens (`[…]`) — a `#footnote[= x]` heading or a
//!   `#link(url)[- x]` list item trigger exactly like a paragraph's own
//!   `= x`. A `Str` node's text is almost always a single line (soft/hard
//!   breaks are separate `Inline` nodes), so escaping the character at
//!   *position 0* of the text — regardless of the true render position —
//!   is a safe superset: it costs a few needless escapes where a `Str`
//!   happens to follow other inline content mid-paragraph, but it never
//!   misses a real one, and it never changes what is rendered (an escaped
//!   printable character reads back identically to the same character
//!   unescaped when it carries no meaning). The one case with an embedded
//!   `\n` (the `parse-internal` fallback that keeps a whole malformed
//!   block as one literal paragraph, `diag-parse-internal`) is covered the
//!   same way: the character right after a `\n` is again a line start.
//!   `=` is escaped unconditionally at a line start (a heading marker is a
//!   *run* of `=`, so escaping only the first one already breaks it for
//!   any run length: `\==x` is not a heading); `+`, `-` and `/` need one
//!   more character of lookahead — they are markers only when the next
//!   character is whitespace (any whitespace: a space, a tab, the line
//!   end) or nothing at all, which leaves a leading `-5` (Typst renders it
//!   with a proper minus sign) or `a--b` (a leading en dash) alone.
//! - A *number* at the start of a line is an enum marker too: digits then
//!   `.` then whitespace (`1. x`, and `0.` alone, whose body is empty —
//!   the `- [ ] 0.` task item of an exam, whose text vanished into an
//!   empty `enum.item`). The backslash goes on the `.`, not on a digit
//!   (`\1` is no escape in Typst): `0\.`. Digits-dot-*digits* is not a
//!   marker, so a leading `3.5 kg` is left alone, and neither is a `.`
//!   whose digits do not start the line (`at 10. o'clock` mid-sentence).
//!   A `)` after the digits is not a marker either: Typst's enum takes
//!   `.` alone (checked on 0.14.2; `1) bar` is text).
//!
//! "Followed by nothing" counts as a line end for all of these: the text
//! of a `Str` that ends there is followed by whatever the writer prints
//! next — a newline, the `]` that closes a content block — and only the
//! second of those is not a line start, so escaping is again the safe
//! superset. It is the case that matters in practice: `#ts-task("open")[0.]`
//! is harmless as written, but the template that unwraps the task leaves
//! `- 0.` on its own line, which is the empty enum item again.
const MARKUP: &[char] = &['\\', '#', '$', '*', '_', '`', '<', '>', '@', '[', ']', '~'];

/// Escapes text for Typst markup mode.
pub fn markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let mut chars = text.chars().peekable();
    // At a line start, in Typst's sense: the start of the text, or the
    // character right after an embedded `\n`.
    let mut leading = true;
    // Digits run from the line start up to here (the number of an enum
    // marker), zero when the line does not start with digits or when the
    // run is already broken.
    let mut digits = 0usize;
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        // A marker ends on whitespace, or on the end of the text: what
        // follows there is whatever the writer prints next — a `\n`, the
        // `]` of a content block — and only the first of those is not a
        // line end, so the end of the text is escaped as one.
        let marks = match next {
            None => true,
            Some(n) => n.is_whitespace(),
        };
        let comment = c == '/' && matches!(next, Some('/') | Some('*'));
        let structural = (leading
            && match c {
                '=' => true,
                '+' | '-' | '/' => marks,
                _ => false,
            })
            // The `.` of an enum marker (`1.`): the backslash goes on the
            // dot, not on a digit, since `\1` is no escape in Typst.
            || (c == '.' && digits > 0 && marks);
        if MARKUP.contains(&c) || comment || structural {
            out.push('\\');
        }
        out.push(c);
        digits = if c.is_ascii_digit() && (leading || digits > 0) {
            digits + 1
        } else {
            0
        };
        // A `Str` node's text normally has no embedded newline (a soft or
        // hard break is a separate `Inline`), except the rare
        // `parse-internal` fallback that keeps a whole malformed block as
        // one literal paragraph (`diag-parse-internal`); either way, the
        // character right after a `\n` is again at a line start.
        leading = c == '\n';
    }
    out
}

/// Escapes text for a Typst string literal (`"…"`).
pub fn string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A Typst label (also a bibliography key). Typst 0.15 reads a label as
/// a run of XID_Continue characters plus `_`, `-`, `:` and `.`; every
/// other character becomes `-`, and a key that lost a character takes a
/// short hash of its original spelling as a suffix so that two ids that
/// differ only in such characters (`日本語 見出し` and `日本語 本文`,
/// `a/b` and `a+b`) never share a label. An accented or non-Latin id is
/// a label as written.
pub fn label(key: &str) -> String {
    let key = key.trim();
    let mut out = String::with_capacity(key.len() + 8);
    let mut lossy = false;
    for c in key.chars() {
        if unicode_ident::is_xid_continue(c) || matches!(c, '_' | '.' | ':' | '-') {
            out.push(c);
        } else {
            out.push('-');
            lossy = true;
        }
    }
    if lossy {
        // FNV-1a over the original bytes: stable across runs and platforms.
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        out.push_str(&format!("-{:06x}", hash & 0xff_ffff));
    }
    out
}

/// A raw span with a backtick fence longer than any run inside
/// (`_raw_inline`); a leading or trailing backtick is padded with a space.
pub fn raw_inline(text: &str) -> String {
    let fence = "`".repeat(longest_backtick_run(text) + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// A fence of at least three backticks, longer than any run in `body`.
pub fn fence_for(body: &str) -> String {
    "`".repeat((longest_backtick_run(body) + 1).max(3))
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_specials() {
        // `test_escaper_escapes_markup_specials` of TeXSmith.
        assert_eq!(
            markup("# * _ ` < > @ [ ] $ \\"),
            "\\# \\* \\_ \\` \\< \\> \\@ \\[ \\] \\$ \\\\"
        );
        assert_eq!(markup("plain text, 50%"), "plain text, 50%");
    }

    #[test]
    fn escapes_tilde() {
        assert_eq!(markup("a~b"), "a\\~b");
    }

    #[test]
    fn escapes_comment_openers_anywhere() {
        // `//` and `/*` swallow the rest of the line/a block if unescaped,
        // wherever they occur, not just at a line start.
        assert_eq!(
            markup("Ratio a // b and the rest"),
            "Ratio a \\// b and the rest"
        );
        assert_eq!(markup("a /* b */ c"), "a \\/\\* b \\*/ c");
        // Mid-word, a lone slash needs nothing.
        assert_eq!(markup("a/b"), "a/b");
    }

    #[test]
    fn escapes_leading_structural_markers() {
        // At the start of the text (a paragraph, or any fresh markup
        // scope: a footnote, a link's content, a table cell), these read
        // as a heading, a list/enum item or a term-list item.
        assert_eq!(markup("= heading"), "\\= heading");
        assert_eq!(markup("== heading level 2"), "\\== heading level 2");
        assert_eq!(markup("- item"), "\\- item");
        assert_eq!(markup("+ item"), "\\+ item");
        assert_eq!(markup("/ term: description"), "\\/ term: description");
        // Not at the start: harmless, left alone.
        assert_eq!(markup("a = b - c + d / e"), "a = b - c + d / e");
        // No trailing space: not a marker, left alone (keeps Typst's own
        // minus-sign and en/em-dash typography working).
        assert_eq!(markup("-5"), "-5");
        assert_eq!(markup("+5"), "+5");
        assert_eq!(markup("/nospace"), "/nospace");
        assert_eq!(markup("a--b"), "a--b");
        // `=` is escaped unconditionally when leading: a run of `=`, not
        // just one, opens a heading, so breaking the first is enough and
        // simpler than counting the run.
        assert_eq!(markup("=5"), "\\=5");
    }

    #[test]
    fn escapes_leading_enum_numbers() {
        // Digits then `.` then whitespace is an enum marker, so the text
        // of the item is eaten: `0.` alone became `enum.item(number: 0,
        // body: [])` and nothing was printed (the `- [ ] 0.` task items
        // of an exam). The backslash goes on the dot: `\1` is no escape.
        assert_eq!(markup("0."), "0\\.");
        assert_eq!(markup("1. foo"), "1\\. foo");
        assert_eq!(markup("12.\tfoo"), "12\\.\tfoo");
        assert_eq!(markup("2. bar\n3. baz"), "2\\. bar\n3\\. baz");
        // Digits then `.` then a digit is a decimal number, not a marker.
        assert_eq!(markup("3.5 kg"), "3.5 kg");
        assert_eq!(markup("1.2.3 released"), "1.2.3 released");
        // The digits must start the line: a sentence's own full stop, or
        // a number in the middle of it, is left alone.
        assert_eq!(markup("at 10. o'clock"), "at 10. o'clock");
        assert_eq!(markup("Bonn, 1949. Later"), "Bonn, 1949. Later");
        // `)` is not an enum marker in Typst (0.14.2: `1) bar` is text).
        assert_eq!(markup("1) bar"), "1) bar");
    }

    #[test]
    fn escapes_markers_before_any_whitespace_or_the_end() {
        // A marker ends on any whitespace, not just a space; and the end
        // of the text is a line end too, since the writer prints a `\n`
        // there more often than the `]` of a content block.
        assert_eq!(markup("-\titem"), "\\-\titem");
        assert_eq!(markup("+\nitem"), "\\+\nitem");
        assert_eq!(markup("/\tterm: description"), "\\/\tterm: description");
        assert_eq!(markup("-"), "\\-");
    }

    #[test]
    fn strings_labels_fences() {
        assert_eq!(string("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(label(" ein05 "), "ein05");
        assert_eq!(label("fig:a.b_c-d"), "fig:a.b_c-d");
        // Unicode letters and combining marks are label characters (typst
        // 0.15 lexer, verified: `<café>`, `<日本語-見出し>`, `<e\u{301}>`
        // compile); a superscript digit is not.
        assert_eq!(label("café-au-lait"), "café-au-lait");
        assert_eq!(label("日本語-見出し"), "日本語-見出し");
        assert_eq!(label("e\u{301}"), "e\u{301}");
        // A lost character leaves a stable hash behind, so two ids that
        // differ only there keep distinct labels.
        let a = label("fig:boot loop");
        let b = label("fig:boot/loop");
        assert!(a.starts_with("fig:boot-loop-") && a.len() == "fig:boot-loop-".len() + 6);
        assert_ne!(a, b);
        assert_eq!(a, label("fig:boot loop"), "deterministic");
        assert_ne!(label("a²"), "a²");
        assert_eq!(raw_inline("x"), "`x`");
        assert_eq!(raw_inline("a`b"), "``a`b``");
        assert_eq!(raw_inline("`a"), "`` `a ``");
        assert_eq!(fence_for("x"), "```");
        assert_eq!(fence_for("```x"), "````");
    }
}
