//! Small parsers for the heads the tokenizer keeps raw: attribute lists,
//! role heads, reference items, fence info strings, container and
//! admonition info. Spec §Lexical grammar.

use tmark_ir::{Attrs, RefItem, SubSpan};
use tmark_markdown::tmark::{is_ident_byte, is_ident_start};

/// Splits `s` into whitespace-separated tokens, keeping `"…"` values
/// whole; inside quotes a backslash protects the next character (spec
/// §Lexical grammar: `"(?:[^"\\]|\\.)*"`).
fn tokens(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut quoted = false;
        while i < bytes.len() && (quoted || !bytes[i].is_ascii_whitespace()) {
            if quoted && bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                quoted = !quoted;
            }
            i += 1;
        }
        out.push(&s[start..i]);
    }
    out
}

fn is_ident(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty() && is_ident_start(bytes[0]) && bytes[1..].iter().all(|b| is_ident_byte(*b))
}

fn is_key(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(is_ident_byte)
}

/// A quoted value without its quotes, `\"` and `\\` decoded; a bare value
/// as is.
fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some(n @ ('"' | '\\')) => out.push(n),
                    Some(n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        value.to_string()
    }
}

/// `key=value` with a quoted or bare value.
fn key_value(token: &str) -> Option<(String, String)> {
    let (key, value) = token.split_once('=')?;
    if !is_key(key) || value.is_empty() {
        return None;
    }
    Some((key.to_string(), unquote(value)))
}

/// Byte offset of `token` inside `s`, of which it is a slice.
fn offset_in(s: &str, token: &str) -> u32 {
    (token.as_ptr() as usize - s.as_ptr() as usize) as u32
}

/// Whether the inside of an attribute list starts with the deprecated
/// Python-Markdown colon (`{: .cls}`, spec §Attributes; Appendix
/// "Deprecation schedule").
pub fn has_attr_colon(s: &str) -> bool {
    s.trim_start().starts_with(':')
}

/// Parses the inside of an attribute list (`#id .class key=value`), with
/// or without the deprecated leading colon (`: #id .class`, spec §Lexical
/// grammar: `\{:?` in place of `\{`). `None` when a token is not an
/// attribute: the group is not an attribute list (spec: "there are no
/// bare-word attributes"). `id_span` is relative to `s`; the caller
/// relocates it (`Lowerer::relocate_attrs`) or drops it.
pub fn parse_attrs(s: &str) -> Option<Attrs> {
    let mut attrs = Attrs::new();
    let body = match s.trim_start().strip_prefix(':') {
        Some(rest) => rest,
        None => s,
    };
    let list = tokens(body);
    if list.is_empty() {
        return None;
    }
    for token in list {
        if let Some(id) = token.strip_prefix('#') {
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| is_ident_byte(b) || matches!(b, b':' | b'.'))
            {
                return None;
            }
            attrs.id = Some(id.to_string());
            let start = offset_in(s, token) + 1;
            attrs.id_span = Some(SubSpan::new(
                Default::default(),
                start,
                start + id.len() as u32,
            ));
        } else if let Some(class) = token.strip_prefix('.') {
            if !is_key(class) {
                return None;
            }
            attrs.classes.push(class.to_string());
        } else if let Some((key, value)) = key_value(token) {
            attrs.kv.push((key, value));
        } else {
            return None;
        }
    }
    Some(attrs)
}

/// The canonical text of an attribute list, braces included: `#id`, then
/// the classes, then the keys in source order, a value quoted when it
/// holds whitespace, `}`, `"`, `=`, `\` or is empty (the printer's rule,
/// `tmark-fmt::attrs`). For the fix of the deprecated colon form and for
/// literal fallbacks.
pub fn attrs_text(attrs: &Attrs) -> String {
    let mut parts = Vec::new();
    if let Some(id) = &attrs.id {
        parts.push(format!("#{id}"));
    }
    for class in &attrs.classes {
        parts.push(format!(".{class}"));
    }
    for (k, v) in &attrs.kv {
        let quoted = v.is_empty()
            || v.contains(|c: char| c.is_whitespace() || matches!(c, '}' | '"' | '=' | '\\'));
        if quoted {
            parts.push(format!(
                "{k}=\"{}\"",
                v.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        } else {
            parts.push(format!("{k}={v}"));
        }
    }
    format!("{{{}}}", parts.join(" "))
}

/// A parsed role head: `{name positional key=value}`.
#[derive(Debug, PartialEq)]
pub struct RoleHead {
    pub name: String,
    /// Deprecated `name:registry` suffix (spec Appendix "Deprecation schedule").
    pub registry_suffix: Option<String>,
    pub positional: Option<String>,
    pub kv: Vec<(String, String)>,
}

/// Parses the inside of a role head. `None` when it is not one.
pub fn parse_role_head(s: &str) -> Option<RoleHead> {
    let list = tokens(s);
    let first = *list.first()?;
    let (name, registry_suffix) = match first.split_once(':') {
        Some((name, suffix)) if is_ident(name) && is_key(suffix) => {
            (name.to_string(), Some(suffix.to_string()))
        }
        None if is_ident(first) => (first.to_string(), None),
        _ => return None,
    };
    let mut positional = None;
    let mut kv = Vec::new();
    for token in &list[1..] {
        if let Some(pair) = key_value(token) {
            kv.push(pair);
        } else if positional.is_none() && kv.is_empty() && !token.contains('=') {
            positional = Some(token.to_string());
        } else {
            return None;
        }
    }
    Some(RoleHead {
        name,
        registry_suffix,
        positional,
        kv,
    })
}

/// The `key`/`id` production of spec §Identifiers as a bracketed item
/// spells it: a letter or a digit first (Zotero's `1RgTv`, challenge
/// C27), a letter or a digit last, `_ : . - /` inside; an all-digit item
/// is never a key.
fn is_ref_key(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && !bytes.iter().all(u8::is_ascii_digit)
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-' | b'/'))
}

/// Parses the items of a bracketed reference (`see ein05, pp. 33-35; -AI2027`):
/// Pandoc's item grammar, one item per `;`, each with an optional prefix, an
/// optional flag (`-` suppresses the author, `+` makes the item narrative,
/// spec §Cite), the key and an optional suffix after the first `,`. A `@`
/// before a key (Pandoc's `[@key]` import form) is accepted and dropped.
/// `key_span` is relative to `inner`; the caller relocates it.
pub fn parse_ref_items(inner: &str) -> Vec<RefItem> {
    let mut out = Vec::new();
    let mut item_start = 0usize;
    for raw in inner.split(';') {
        let base = item_start;
        item_start += raw.len() + 1;
        let lead = raw.len() - raw.trim_start().len();
        let item = raw.trim();
        let item_at = base + lead;
        let (head, suffix) = match item.split_once(',') {
            Some((head, suffix)) => (head.trim_end(), Some(suffix.trim().to_string())),
            None => (item, None),
        };
        let word_at = head.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let word = &head[word_at..];
        let mut suppress_author = false;
        let mut narrative = false;
        let mut candidate = word;
        let mut skipped = 0;
        if let Some(rest) = candidate.strip_prefix('-') {
            suppress_author = true;
            candidate = rest;
            skipped += 1;
        } else if let Some(rest) = candidate.strip_prefix('+') {
            narrative = true;
            candidate = rest;
            skipped += 1;
        }
        if let Some(rest) = candidate.strip_prefix('@') {
            candidate = rest;
            skipped += 1;
        }
        let (prefix, key, key_at, key_len, suffix) = if !word.is_empty() && is_ref_key(candidate) {
            let words = head[..word_at].trim_end();
            (
                (!words.is_empty()).then(|| words.split_whitespace().collect::<Vec<_>>().join(" ")),
                candidate.to_string(),
                item_at + word_at + skipped,
                candidate.len(),
                suffix.filter(|s| !s.is_empty()),
            )
        } else {
            // Not a key: the whole item is the key, as typed (the suffix
            // is part of it, so it is not repeated).
            suppress_author = false;
            narrative = false;
            (None, item.replace('@', ""), item_at, item.len(), None)
        };
        out.push(RefItem {
            prefix,
            suppress_author,
            narrative,
            key,
            key_span: SubSpan::new(Default::default(), key_at as u32, (key_at + key_len) as u32),
            suffix,
        });
    }
    out
}

/// A parsed fence info string (family 4).
#[derive(Debug, PartialEq)]
pub struct FenceInfo {
    pub lang: String,
    pub node: Option<String>,
    /// `key=value` options, plus the classes and id of a trailing attribute
    /// list (`{.snippet caption="…"}`, design 12 C28). `id_span` is unset.
    pub attrs: Attrs,
    /// Text of the info string that did not parse as options, kept for the
    /// listing.
    pub rest: Option<String>,
    /// The info string was superfences' braces-only spelling
    /// (`{ .c .annotate }`), whose first class is the language: the
    /// caller reports it `deprecated` (spec §Lexical grammar, family 4).
    pub braced: bool,
}

/// Parses `lang [node] [key=value…] [{attrs}]`: bare `key=value` options
/// (spec §Lexical grammar, family 4) and, at the end, an attribute list in
/// braces with the C1 value grammar, which is the only spelling that can
/// carry classes (`{.snippet caption="Title" width="60%"}`).
///
/// `pymdownx.superfences` also spells the whole info string as one
/// attribute list with no bare word before it (`{ .c .annotate }`, the
/// form Material documents for annotations). CommonMark splits an info
/// string on its first whitespace, so that spelling reaches this function
/// as a `lang` of `{` or `{.c`: it is recombined and read as the
/// attribute list it is, the first class becoming the language. A
/// language of `{` is never returned.
pub fn parse_fence_info(lang: Option<&str>, meta: Option<&str>) -> Option<FenceInfo> {
    let lang = lang?.trim();
    if lang.is_empty() {
        return None;
    }
    if lang.starts_with('{') {
        return braces_only(lang, meta);
    }
    let mut info = FenceInfo {
        lang: lang.to_string(),
        node: None,
        attrs: Attrs::new(),
        rest: None,
        braced: false,
    };
    let Some(meta) = meta else {
        return Some(info);
    };
    let meta = meta.trim();
    let (plain, braced) = match meta.find('{') {
        Some(at) if meta.ends_with('}') => (&meta[..at], Some(&meta[at + 1..meta.len() - 1])),
        _ => (meta, None),
    };
    let list = tokens(plain);
    let mut index = 0;
    if let Some(first) = list.first() {
        if tmark_ir::registry::node_word(first).is_some() {
            info.node = Some((*first).to_string());
            index = 1;
        }
    }
    let mut rest = Vec::new();
    for token in &list[index..] {
        match key_value(token) {
            Some(pair) => info.attrs.kv.push(pair),
            None => rest.push(*token),
        }
    }
    match braced.map(parse_attrs) {
        Some(Some(attrs)) => {
            info.attrs.id = attrs.id;
            info.attrs.classes = attrs.classes;
            info.attrs.kv.extend(attrs.kv);
        }
        // Not an attribute list: literal text of the info string.
        Some(None) => rest.push(&meta[meta.find('{').unwrap_or(0)..]),
        None => {}
    }
    if !rest.is_empty() {
        info.rest = Some(rest.join(" "));
    }
    Some(info)
}

/// The braces-only info string of `pymdownx.superfences`: the whole
/// string is one attribute list and its first class is the language
/// (`{ .c .annotate }` → `c {.annotate}`). A group that does not parse as
/// an attribute list is no info string at all — the fence is a plain
/// listing with no language, never one whose language is `{`. A group
/// with no class leaves `lang` empty: the list has no language to attach
/// to and the caller reports `attr-no-host`.
fn braces_only(lang: &str, meta: Option<&str>) -> Option<FenceInfo> {
    let mut whole = lang.to_string();
    if let Some(meta) = meta {
        whole.push(' ');
        whole.push_str(meta);
    }
    let whole = whole.trim();
    let inner = whole.strip_prefix('{')?.strip_suffix('}')?;
    let mut attrs = parse_attrs(inner)?;
    attrs.id_span = None;
    let lang = match attrs.classes.is_empty() {
        true => String::new(),
        false => attrs.classes.remove(0),
    };
    Some(FenceInfo {
        lang,
        node: None,
        attrs,
        rest: None,
        braced: true,
    })
}

/// Splits a container info string into its name and the raw attribute list.
pub fn parse_container_info(info: &str) -> (String, Option<Attrs>, bool) {
    let info = info.trim();
    let (name, rest) = match info.find(|c: char| c.is_ascii_whitespace()) {
        Some(at) => (&info[..at], info[at..].trim()),
        None => (info, ""),
    };
    let mut valid = true;
    let attrs = if rest.is_empty() {
        None
    } else if rest.starts_with('{') && rest.ends_with('}') {
        let parsed = parse_attrs(&rest[1..rest.len() - 1]);
        valid = parsed.is_some();
        parsed
    } else {
        valid = false;
        None
    };
    (name.to_string(), attrs, valid)
}

/// Splits `type class… "Title"` of a PyMdownX admonition. The title comes
/// with its byte offset in `info`: it is a verbatim slice of the marker
/// line, so what is parsed from it carries spans of the file (spec
/// §Round-trip and source spans).
pub fn parse_admonition_info(info: &str) -> (String, Vec<String>, Option<(String, usize)>) {
    let trimmed = info.trim();
    let (head, title) = match trimmed.find('"') {
        Some(at) => {
            let inner = trimmed[at..].trim().trim_matches('"');
            (
                trimmed[..at].trim(),
                Some((inner.to_string(), offset_in(info, inner) as usize)),
            )
        }
        None => (trimmed, None),
    };
    let mut words = head.split_whitespace().map(str::to_string);
    let kind = words.next().unwrap_or_default();
    (kind, words.collect(), title)
}

/// Byte offset, inside the text of an attribute list, of the value of
/// `key` — of the value itself, quotes off. `None` when the list has no
/// such key or when its value is not a verbatim slice of the text (a
/// quoted value with a `\"` or a `\\` in it, which `unquote` decodes):
/// only a verbatim value can carry spans of the file.
pub fn attr_value_offset(s: &str, key: &str) -> Option<usize> {
    let body = match s.trim_start().strip_prefix(':') {
        Some(rest) => rest,
        None => s,
    };
    for token in tokens(body) {
        let Some((name, value)) = token.split_once('=') else {
            continue;
        };
        if name != key || value.is_empty() {
            continue;
        }
        let inner = match value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            true => &value[1..value.len() - 1],
            false => value,
        };
        if inner.contains('\\') {
            return None;
        }
        return Some(offset_in(s, inner) as usize);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs() {
        let attrs = parse_attrs("#sec:intro .draft lang=en title=\"a b\"").unwrap();
        assert_eq!(attrs.id.as_deref(), Some("sec:intro"));
        assert_eq!(attrs.classes, vec!["draft"]);
        assert_eq!(attrs.get("lang"), Some("en"));
        assert_eq!(attrs.get("title"), Some("a b"));
        let attrs = parse_attrs(r#"k="a \"b\" c" j="x}y" l="p\\q" m="\n""#).unwrap();
        assert_eq!(attrs.get("k"), Some(r#"a "b" c"#));
        assert_eq!(attrs.get("j"), Some("x}y"));
        assert_eq!(attrs.get("l"), Some(r"p\q"));
        assert_eq!(attrs.get("m"), Some(r"\n"), "only `\\\"` and `\\\\` decode");
        assert!(parse_attrs("collapsed").is_none(), "no bare words");
        // The deprecated Python-Markdown colon (spec §Attributes).
        let attrs = parse_attrs(": #sec:boot .draft").unwrap();
        assert_eq!(attrs.id.as_deref(), Some("sec:boot"));
        assert_eq!(attrs.classes, vec!["draft"]);
        assert_eq!(parse_attrs(":.thin").unwrap().classes, vec!["thin"]);
        assert!(has_attr_colon(": .thin") && !has_attr_colon(".thin"));
        assert!(parse_attrs(":").is_none());
        assert_eq!(attrs_text(&attrs), "{#sec:boot .draft}");
        let mut attrs = Attrs::new();
        attrs.kv.push(("title".into(), "a \"b\"".into()));
        assert_eq!(attrs_text(&attrs), "{title=\"a \\\"b\\\"\"}");
        assert!(parse_attrs("").is_none());
        assert!(parse_attrs("aside side=left").is_none());
    }

    #[test]
    fn role_heads() {
        let head = parse_role_head("aside side=left").unwrap();
        assert_eq!(head.name, "aside");
        assert_eq!(head.kv, vec![("side".to_string(), "left".to_string())]);
        let head = parse_role_head("raw latex").unwrap();
        assert_eq!(head.positional.as_deref(), Some("latex"));
        let head = parse_role_head("index:physics").unwrap();
        assert_eq!(head.registry_suffix.as_deref(), Some("physics"));
        assert!(parse_role_head("#id").is_none());
        assert!(parse_role_head("a b c").is_none(), "one positional at most");
    }

    #[test]
    fn ref_items() {
        let items = parse_ref_items("see ein05, pp. 33-35; -AI2027, ch. 1; fig:a");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].prefix.as_deref(), Some("see"));
        assert_eq!(items[0].key, "ein05");
        assert_eq!(items[0].suffix.as_deref(), Some("pp. 33-35"));
        assert!(items[1].suppress_author);
        assert_eq!(items[1].key, "AI2027");
        assert_eq!(items[2].key, "fig:a");
        assert!(items[2].prefix.is_none() && items[2].suffix.is_none());
        let inner = "see ein05, pp. 33-35; -AI2027, ch. 1; fig:a";
        let spans: Vec<&str> = items
            .iter()
            .map(|i| &inner[i.key_span.0.start as usize..i.key_span.0.end as usize])
            .collect();
        assert_eq!(spans, ["ein05", "AI2027", "fig:a"]);
        let items = parse_ref_items("@ab; -@bc, p. 1; not a key!");
        assert_eq!(items[0].key, "ab");
        assert_eq!((items[0].key_span.0.start, items[0].key_span.0.end), (1, 3));
        assert!(items[1].suppress_author);
        assert_eq!(items[1].key, "bc");
        assert_eq!((items[1].key_span.0.start, items[1].key_span.0.end), (7, 9));
        assert_eq!(items[2].key, "not a key!");
        assert_eq!(
            (items[2].key_span.0.start, items[2].key_span.0.end),
            (17, 27)
        );
        // A one-letter key is not a key (spec grammar: two characters at
        // least); the whole item is, without a repeated suffix.
        let items = parse_ref_items("-@b, p. 3");
        assert_eq!(items[0].key, "-b, p. 3");
        assert!(items[0].suffix.is_none() && !items[0].suppress_author);
        // `+` is the narrative flag; one flag per item, so `+-` is no key.
        let items = parse_ref_items("see +ein05, p. 3; +@ko20; +-ko20");
        assert!(items[0].narrative && !items[0].suppress_author);
        assert_eq!(items[0].prefix.as_deref(), Some("see"));
        assert_eq!(items[0].key, "ein05");
        assert_eq!(
            (items[0].key_span.0.start, items[0].key_span.0.end),
            (5, 10)
        );
        assert_eq!(items[0].suffix.as_deref(), Some("p. 3"));
        assert!(items[1].narrative);
        assert_eq!(items[1].key, "ko20");
        assert!(!items[2].narrative && items[2].key == "+-ko20");
        // A digit-initial key (spec §Identifiers, C27) keeps its locator
        // and prefix out of the key; an all-digit item is no key.
        let items = parse_ref_items("7HA7H, p. 3; see 1RgTv; 12, p. 4");
        assert_eq!(items[0].key, "7HA7H");
        assert_eq!(items[0].suffix.as_deref(), Some("p. 3"));
        assert_eq!(items[1].prefix.as_deref(), Some("see"));
        assert_eq!(items[1].key, "1RgTv");
        assert_eq!(items[2].key, "12, p. 4");
    }

    #[test]
    fn attrs_id_span() {
        let attrs = parse_attrs(" .draft #sec:intro lang=en").unwrap();
        let span = attrs.id_span.unwrap().0;
        assert_eq!((span.start, span.end), (9, 18));
        let attrs = parse_attrs(": #id").unwrap();
        let span = attrs.id_span.unwrap().0;
        assert_eq!((span.start, span.end), (3, 5), "relative to the whole text");
    }

    #[test]
    fn fence_info_braces_only() {
        // superfences' spelling: CommonMark hands the first word as the
        // language, so `{` and `{.c` both have to be recombined.
        for (lang, meta) in [
            ("{", Some(".c .annotate }")),
            ("{.c", Some(".annotate}")),
            ("{.c", Some(".annotate }")),
            ("{", Some(": .c .annotate}")),
        ] {
            let info = parse_fence_info(Some(lang), meta).unwrap();
            assert_eq!(info.lang, "c", "{lang} {meta:?}");
            assert_eq!(info.attrs.classes, vec!["annotate"], "{lang} {meta:?}");
            assert!(info.braced);
            assert!(info.node.is_none());
        }
        let info = parse_fence_info(Some("{.c}"), None).unwrap();
        assert_eq!(info.lang, "c");
        assert!(info.attrs.classes.is_empty());
        // An id and an option travel with it; the first class still wins.
        let info = parse_fence_info(Some("{#x"), Some(".c title=\"a b\"}")).unwrap();
        assert_eq!(info.lang, "c");
        assert_eq!(info.attrs.id.as_deref(), Some("x"));
        assert_eq!(info.attrs.get("title"), Some("a b"));
        // No class: no language, and the caller reports `attr-no-host`.
        let info = parse_fence_info(Some("{"), Some("title=\"only\" }")).unwrap();
        assert!(info.lang.is_empty());
        assert!(info.braced);
        // Never a language of `{`.
        for (lang, meta) in [
            ("{", None),
            ("{", Some("not an attribute list")),
            ("{.c", Some("no closing brace")),
        ] {
            assert!(
                parse_fence_info(Some(lang), meta).map_or(true, |i| i.lang != "{"),
                "{lang} {meta:?}"
            );
        }
    }

    #[test]
    fn fence_info() {
        let info = parse_fence_info(Some("python"), Some("image include=\"plot.py\"")).unwrap();
        assert_eq!(info.node.as_deref(), Some("image"));
        assert_eq!(
            info.attrs.kv,
            vec![("include".to_string(), "plot.py".to_string())]
        );
        let info = parse_fence_info(Some("yaml"), Some("table")).unwrap();
        assert_eq!(info.node.as_deref(), Some("table"));
        let info = parse_fence_info(Some("js"), Some("title=\"a.js\" linenums=\"1\"")).unwrap();
        assert!(info.node.is_none());
        assert_eq!(info.attrs.kv.len(), 2);
        assert!(parse_fence_info(None, None).is_none());
        // A trailing attribute list (C28): classes and quoted values.
        let info = parse_fence_info(
            Some("md"),
            Some("{.snippet caption=\"A title\" width=\"60%\"}"),
        )
        .unwrap();
        assert_eq!(info.attrs.classes, vec!["snippet"]);
        assert_eq!(info.attrs.get("caption"), Some("A title"));
        assert_eq!(info.attrs.get("width"), Some("60%"));
        assert!(info.rest.is_none());
        let info = parse_fence_info(Some("mermaid"), Some("{width=80%}")).unwrap();
        assert_eq!(info.attrs.get("width"), Some("80%"));
        let info = parse_fence_info(Some("python"), Some("image {#fig:x .wide} ")).unwrap();
        assert_eq!(info.node.as_deref(), Some("image"));
        assert_eq!(info.attrs.id.as_deref(), Some("fig:x"));
        let info = parse_fence_info(Some("c"), Some("{not attrs}")).unwrap();
        assert!(info.attrs.is_empty());
        assert_eq!(info.rest.as_deref(), Some("{not attrs}"));
    }

    #[test]
    fn container_and_admonition_info() {
        let (name, attrs, valid) = parse_container_info("warning {title=\"LaTeX toolchain\"}");
        assert_eq!(name, "warning");
        assert_eq!(attrs.unwrap().get("title"), Some("LaTeX toolchain"));
        assert!(valid);
        let (kind, classes, title) = parse_admonition_info("note inline end \"Folded\"");
        assert_eq!(kind, "note");
        assert_eq!(classes, vec!["inline", "end"]);
        assert_eq!(title, Some(("Folded".to_string(), 17)));
        // The offset is the one of the title in the info as given, blanks
        // and all: a span of the marker line, not of the trimmed info.
        let info = "  exercise \"#(ex:un) : T\"  ";
        let (_, _, title) = parse_admonition_info(info);
        let (text, at) = title.unwrap();
        assert_eq!(&info[at..at + text.len()], "#(ex:un) : T");
    }

    #[test]
    fn attribute_values_are_located() {
        let s = "#a .b title=\"#(ex:un) : T\" lang=en";
        let at = attr_value_offset(s, "title").unwrap();
        assert_eq!(&s[at..at + "#(ex:un) : T".len()], "#(ex:un) : T");
        let at = attr_value_offset(s, "lang").unwrap();
        assert_eq!(&s[at..], "en");
        assert_eq!(attr_value_offset(s, "kind"), None);
        // `unquote` decodes the escapes, so the value is not a slice.
        assert_eq!(attr_value_offset(r#"title="a \"b\"""#, "title"), None);
        // The deprecated leading colon does not move the value.
        let s = ": title=\"T\"";
        let at = attr_value_offset(s, "title").unwrap();
        assert_eq!(&s[at..at + 1], "T");
    }
}
