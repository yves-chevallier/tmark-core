//! LaTeX escaping: the tables and the order of TeXSmith's
//! `writers/latex/escaper.py` (`escape_latex_chars`, `prepare_plain_text`).
//!
//! Order (`escaper.py:301-313`): smart quotes and the ellipsis to ASCII,
//! Unicode dashes to `--`/`---`, the character and symbol maps, then
//! Unicode superscript and subscript runs to `\textsuperscript{…}` /
//! `\textsubscript{…}`. Math typed in prose is *not* scanned (decision in
//! writers-and-passes.md §2: `$…$` is a `Math` node under TMark).

use crate::common::text::{self, DASHES, PUNCTUATION, SUBSCRIPTS, SUPERSCRIPTS};

/// `escaper.py:23-34`.
fn special(c: char) -> Option<&'static str> {
    Some(match c {
        '&' => "\\&",
        '%' => "\\%",
        '#' => "\\#",
        '$' => "\\$",
        '_' => "\\_",
        '^' => "\\^{}",
        '{' => "\\{",
        '}' => "\\}",
        '~' => "\\textasciitilde{}",
        '\\' => "\\textbackslash{}",
        _ => return None,
    })
}

/// `escaper.py:36-49`: arrows and relations become math in text.
fn symbol(c: char) -> Option<&'static str> {
    Some(match c {
        '→' => "\\(\\rightarrow\\)",
        '←' => "\\(\\leftarrow\\)",
        '⇒' => "\\(\\Rightarrow\\)",
        '⇐' => "\\(\\Leftarrow\\)",
        '≥' => "\\(\\geq\\)",
        '≤' => "\\(\\leq\\)",
        '≠' => "\\(\\neq\\)",
        '≈' => "\\(\\approx\\)",
        '±' => "\\(\\pm\\)",
        '×' => "\\(\\times\\)",
        '÷' => "\\(\\div\\)",
        '∞' => "\\(\\infty\\)",
        _ => return None,
    })
}

/// `escape_latex_chars`: the special characters and the symbol map, nothing
/// else. Used for text that goes into a macro argument that is not prose
/// (a URL, a code span, a key value).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if let Some(s) = symbol(c) {
            out.push_str(s);
        } else if let Some(s) = special(c) {
            out.push_str(s);
        } else {
            out.push(c);
        }
    }
    out
}

/// `prepare_plain_text`: what a `Str` in prose becomes.
pub fn prose(text: &str) -> String {
    let mut normalised = String::with_capacity(text.len());
    for c in text.chars() {
        if let Some(s) = text::lookup(PUNCTUATION, c) {
            normalised.push_str(s);
        } else if let Some(s) = text::lookup(DASHES, c) {
            normalised.push_str(s);
        } else {
            normalised.push(c);
        }
    }
    let escaped = escape(&normalised);
    let escaped = runs(&escaped, SUPERSCRIPTS, "textsuperscript");
    runs(&escaped, SUBSCRIPTS, "textsubscript")
}

/// Replaces every run of characters in `table` by `\command{bases}`.
fn runs(text: &str, table: &[(char, &'static str)], command: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        match text::lookup(table, c) {
            Some(base) => run.push_str(base),
            None => {
                if !run.is_empty() {
                    out.push('\\');
                    out.push_str(command);
                    out.push('{');
                    out.push_str(&run);
                    out.push('}');
                    run.clear();
                }
                out.push(c);
            }
        }
    }
    if !run.is_empty() {
        out.push('\\');
        out.push_str(command);
        out.push('{');
        out.push_str(&run);
        out.push('}');
    }
    out
}

/// Brace-protects a `[` that opens `text`, in place.
///
/// Every command that can stand right before a table cell or a text run
/// takes an optional argument and scans for it with `\@ifnextchar[`,
/// which skips spaces *and* line ends: the row break `\\[⟨dimen⟩]`,
/// booktabs' `\toprule[⟨wd⟩]`, `\midrule`, `\bottomrule`,
/// `\cmidrule[⟨wd⟩]` and `\addlinespace[⟨dimen⟩]`. A cell whose first
/// character is `[` is then eaten as that argument and TeX reports
/// `Missing number, treated as zero`.
///
/// The guard goes on the *content*, not on the command: `{[}` typesets
/// the same character and stops the scan wherever the cell ends up.
/// Guarding the command instead (`\\{}`, or `\tabularnewline`, which
/// takes the same optional argument) is wrong here — a group between the
/// row break and the next cell makes `\multicolumn` no longer the first
/// token of its cell, which is an error, and a spanning cell of a
/// `yaml table` opens with exactly that.
pub fn guard_bracket(text: &mut String) {
    if text.starts_with('[') {
        text.replace_range(0..1, "{[}");
    }
}

/// A URL for `\href`: percent-encode what `requests.utils.requote_uri`
/// encodes (bytes outside the unreserved and reserved sets, leaving
/// existing `%XX` escapes alone), then escape for LaTeX
/// (`formatter.py:142`).
pub fn url(raw: &str) -> String {
    escape(&requote(raw))
}

/// `requote_uri`: `%XX` sequences are kept, everything not in
/// `A-Za-z0-9-._~!#$&'()*+,/:;=?@[]%` is percent-encoded.
pub fn requote(raw: &str) -> String {
    const SAFE: &str = "!#$&'()*+,/:;=?@[]%-._~";
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&raw[i..i + 3]);
            i += 3;
            continue;
        }
        if b.is_ascii_alphanumeric() || SAFE.as_bytes().contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specials() {
        assert_eq!(
            escape("& % # $ _ ^ { } ~ \\"),
            "\\& \\% \\# \\$ \\_ \\^{} \\{ \\} \\textasciitilde{} \\textbackslash{}"
        );
        assert_eq!(escape("café — 100%"), "café — 100\\%");
    }

    #[test]
    fn symbols_become_math() {
        assert_eq!(escape("a → b ≤ c"), "a \\(\\rightarrow\\) b \\(\\leq\\) c");
        assert_eq!(escape("±∞"), "\\(\\pm\\)\\(\\infty\\)");
    }

    #[test]
    fn prose_order() {
        // Quotes and dashes first, then escaping, then script runs.
        assert_eq!(prose("‘a’ “b” c…"), "`a' ``b'' c...");
        assert_eq!(prose("1–2 — 3"), "1--2 --- 3");
        assert_eq!(
            prose("x² and H₂O"),
            "x\\textsuperscript{2} and H\\textsubscript{2}O"
        );
        assert_eq!(prose("10⁻³"), "10\\textsuperscript{-3}");
        assert_eq!(prose("vᵦ"), "v\\textsubscript{\\beta}");
        assert_eq!(prose("50% off"), "50\\% off");
        assert_eq!(prose(""), "");
    }

    #[test]
    fn leading_bracket_is_braced() {
        let mut text = String::from("[x] y");
        guard_bracket(&mut text);
        assert_eq!(text, "{[}x] y");
        let mut text = String::from("a [x]");
        guard_bracket(&mut text);
        assert_eq!(text, "a [x]");
        let mut text = String::new();
        guard_bracket(&mut text);
        assert_eq!(text, "");
    }

    #[test]
    fn urls() {
        assert_eq!(
            url("https://en.wikipedia.org/wiki/Albert_Einstein"),
            "https://en.wikipedia.org/wiki/Albert\\_Einstein"
        );
        assert_eq!(requote("https://x.y/a b"), "https://x.y/a%20b");
        assert_eq!(requote("https://x.y/%20é"), "https://x.y/%20%C3%A9");
        assert_eq!(url("https://x.y/?a=1&b=2#c"), "https://x.y/?a=1\\&b=2\\#c");
    }
}
