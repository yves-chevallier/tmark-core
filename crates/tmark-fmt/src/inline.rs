//! Inline nodes to their canonical spelling (spec Table "Inline text nodes",
//! §Roles, §Ref, §Cite, §IndexEntry, §CounterItem, §Aside).

use tmark_ir::{Block, Inline, QuoteKind, RefItem, Side, Target};

use crate::attrs;
use crate::escape::{self, Context};
use crate::mkdocs;
use crate::out::Out;

/// Writes `inlines` at the current position.
pub fn inlines(out: &mut Out, inlines: &[Inline], ctx: Context) {
    for (i, inline) in inlines.iter().enumerate() {
        let next = inlines.get(i + 1).and_then(first_char);
        let after_index = i > 0 && matches!(inlines[i - 1], Inline::IndexEntry(_));
        let ctx = Context {
            block_start: ctx.block_start && i == 0,
            after_index,
            ..ctx
        };
        one(out, inline, ctx, next);
    }
}

/// The first character the next inline prints, when it is plain text.
fn first_char(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Str(s) => s.text.chars().next(),
        Inline::Abbr(a) => a.text.chars().next(),
        Inline::Space(_) => Some(' '),
        Inline::SoftBreak(_) | Inline::LineBreak(_) => Some('\n'),
        _ => None,
    }
}

/// `{name key=value}[content]` roles and their friends.
fn role(out: &mut Out, head: &str, content: &[Inline], ctx: Context) {
    out.push("{");
    out.push(head);
    out.push("}[");
    inlines(
        out,
        content,
        Context {
            in_group: true,
            block_start: false,
            ..ctx
        },
    );
    out.push("]");
}

fn one(out: &mut Out, inline: &Inline, ctx: Context, next: Option<char>) {
    if out.mkdocs().is_some() && mkdocs_inline(out, inline, ctx, next) {
        return;
    }
    match inline {
        Inline::Str(s) => escape::text(out, &s.text, ctx, next),
        Inline::Space(_) => out.push(" "),
        Inline::SoftBreak(_) => out.push("\n"),
        Inline::LineBreak(_) => out.push("\\\n"),
        Inline::Emph(n) => {
            out.push("*");
            inlines(
                out,
                &n.content,
                Context {
                    block_start: false,
                    ..ctx
                },
            );
            out.push("*");
        }
        Inline::Strong(n) => {
            out.push("**");
            inlines(
                out,
                &n.content,
                Context {
                    block_start: false,
                    ..ctx
                },
            );
            out.push("**");
        }
        Inline::Strikeout(n) => role(out, "del", &n.content, ctx),
        Inline::Underline(n) => role(out, "underline", &n.content, ctx),
        Inline::Highlight(n) => role(out, "mark", &n.content, ctx),
        Inline::Subscript(n) => role(out, "sub", &n.content, ctx),
        Inline::Superscript(n) => role(out, "sup", &n.content, ctx),
        Inline::SmallCaps(n) => role(out, "sc", &n.content, ctx),
        Inline::Quoted(n) => {
            let q = match n.kind {
                QuoteKind::Double => "\"",
                QuoteKind::Single => "'",
            };
            out.push(q);
            inlines(
                out,
                &n.content,
                Context {
                    block_start: false,
                    ..ctx
                },
            );
            out.push(q);
        }
        Inline::Code(n) => match &n.lang {
            Some(lang) => {
                out.push("{code lang=");
                out.push(&attrs::value(lang));
                out.push("}[");
                out.push(&group_verbatim(&n.text));
                out.push("]");
            }
            None => code_span(out, &n.text, ctx.in_cell),
        },
        Inline::Math(n) => {
            let fence = if n.display { "$$" } else { "$" };
            out.push(fence);
            out.push(&n.text);
            out.push(fence);
        }
        Inline::Link(n) => link(out, n, ctx),
        Inline::Ref(n) => reference(out, &n.items, n.bracketed),
        Inline::Note(n) => match &n.label {
            Some(label) => {
                out.push("[^");
                out.push(label);
                out.push("]");
            }
            None => {
                // An inline footnote (proposed): print it as a note body.
                out.push("^[");
                for block in &n.content {
                    if let Some(p) = para_like(block) {
                        inlines(
                            out,
                            p,
                            Context {
                                in_group: true,
                                ..ctx
                            },
                        );
                    }
                }
                out.push("]");
            }
        },
        Inline::Image(n) => {
            out.push("![");
            inlines(
                out,
                &n.alt,
                Context {
                    in_group: true,
                    block_start: false,
                    ..ctx
                },
            );
            out.push("](");
            out.push(&destination(&n.src));
            out.push(")");
            attrs::write(out, &n.attrs, "");
        }
        Inline::IndexEntry(n) => {
            let mut head = String::from("index");
            if n.main {
                head.push_str(" main=true");
            }
            if let Some(registry) = &n.registry {
                head.push_str(" registry=");
                head.push_str(&attrs::value(registry));
            }
            out.push("{");
            out.push(&head);
            out.push("}");
            for group in &n.path {
                out.push("[");
                inlines(
                    out,
                    group,
                    Context {
                        in_group: true,
                        block_start: false,
                        ..ctx
                    },
                );
                out.push("]");
            }
        }
        Inline::CounterItem(n) => {
            out.push("{counter}(");
            out.push(&n.prefix);
            out.push(":");
            out.push(&n.key);
            out.push(")");
        }
        Inline::Keystroke(n) => {
            out.push("{keys}[");
            out.push(&n.keys.join("+"));
            out.push("]");
        }
        Inline::Aside(n) => {
            let mut head = String::from("aside");
            if let Some(side) = n.side {
                head.push_str(" side=");
                head.push_str(side_name(side));
            }
            out.push("{");
            out.push(&head);
            out.push("}[");
            for (i, block) in n.content.iter().enumerate() {
                if i > 0 {
                    out.push(" ");
                }
                if let Some(p) = para_like(block) {
                    inlines(
                        out,
                        p,
                        Context {
                            in_group: true,
                            block_start: false,
                            ..ctx
                        },
                    );
                }
            }
            out.push("]");
        }
        Inline::Span(n) => {
            if let Some(kind) = tmark_ir::critic(n) {
                // `Span{.critic}` prints the critic spelling (spec Appendix
                // "PyMdownX compatibility profile", challenge C49): it is
                // the only spelling, and class E under `pymdownx.critic`,
                // so both profiles emit it.
                critic(out, kind, ctx);
                return;
            }
            if let Some(shortcode) = icon_shortcode(n) {
                // `Span{.icon media=web}` prints as the shortcode it holds
                // (spec §Emoji and icon shortcodes).
                out.push(shortcode);
                return;
            }
            out.push("[");
            inlines(
                out,
                &n.content,
                Context {
                    in_group: true,
                    block_start: false,
                    ..ctx
                },
            );
            out.push("]");
            attrs::write(out, &n.attrs, "");
        }
        Inline::Var(n) => {
            out.push("{{ ");
            out.push(&n.path.join("."));
            out.push(" }}");
        }
        Inline::Abbr(n) => escape::text(out, &n.text, ctx, next),
        Inline::Comment(n) => {
            out.push("<!--");
            out.push(&n.text);
            out.push("-->");
        }
        Inline::RawInline(n) => raw_inline(out, n),
        Inline::ProgressBar(n) => progress_bar(out, n),
    }
}

/// `[=45% "label"]{.thin}` (spec §ProgressBar: the PyMdownX percentage
/// spelling is canonical; the label is quoted, quotes inside it are what
/// the recogniser cannot hold, so they are dropped).
pub fn progress_bar(out: &mut Out, n: &tmark_ir::ProgressBar) {
    out.push(&n.head_text());
    attrs::write(out, &n.attrs, "");
}

/// A raw inline: HTML that CommonMark reads as a tag prints as typed
/// (spec §Raw passthrough: "a raw HTML node whose text CommonMark
/// recognises as HTML prints as typed"); anything else is the role.
fn raw_inline(out: &mut Out, n: &tmark_ir::RawInline) {
    if n.format == "html" && is_html_tag(&n.text) {
        out.push(&n.text);
        return;
    }
    out.push("{raw ");
    out.push(&attrs::value(&n.format));
    out.push("}(");
    out.push(&n.text);
    out.push(")");
}

/// Whether `text` is one inline HTML construct as CommonMark's `html_text`
/// reads it: an open tag, a closing tag, a comment, a processing
/// instruction, a declaration or a CDATA section, nothing around it.
pub fn is_html_tag(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'<' || bytes[bytes.len() - 1] != b'>' {
        return false;
    }
    let inner = &text[1..text.len() - 1];
    if inner.starts_with("!--") {
        return inner.ends_with("--") && !inner[3..inner.len() - 2].contains("--");
    }
    if inner.starts_with('?') || inner.starts_with('!') {
        return !inner[1..].contains('>');
    }
    let name = inner.strip_prefix('/').unwrap_or(inner);
    let name_len = name
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if name_len == 0 || !name.as_bytes()[0].is_ascii_alphabetic() {
        return false;
    }
    let rest = &name[name_len..];
    if inner.starts_with('/') {
        return rest.trim().is_empty();
    }
    // Attributes: no `>` or newline-broken quote; the tokenizer accepted it.
    !rest.trim_end_matches('/').contains(['>', '<'])
}

/// The MkDocs spelling of an inline, when the table has one for it
/// (`mkdocs.rs`); `false` falls through to the canonical spelling.
fn mkdocs_inline(out: &mut Out, inline: &Inline, ctx: Context, next: Option<char>) -> bool {
    match inline {
        Inline::Strikeout(n) => mkdocs::delimited(out, "~~", &n.content, ctx, mkdocs::tilde_family),
        Inline::Highlight(n) => {
            mkdocs::delimited(out, "==", &n.content, ctx, mkdocs::equals_family)
        }
        Inline::Subscript(n) => mkdocs::delimited(out, "~", &n.content, ctx, mkdocs::tilde_family),
        Inline::Superscript(n) => {
            mkdocs::delimited(out, "^", &n.content, ctx, mkdocs::caret_family)
        }
        Inline::SmallCaps(n) => mkdocs::smallcaps(out, &n.content, ctx, next),
        Inline::Code(n) => n
            .lang
            .as_deref()
            .is_some_and(|lang| mkdocs::code(out, &n.text, lang, ctx.in_cell)),
        Inline::Ref(n) => mkdocs::reference(out, &n.items),
        Inline::IndexEntry(n) => mkdocs::index(out, n, ctx),
        Inline::CounterItem(n) => mkdocs::counter(out, n),
        Inline::Keystroke(n) => mkdocs::keys(out, &n.keys),
        Inline::Aside(n) => mkdocs::aside(out, n, ctx),
        Inline::Span(n) => mkdocs::anchor(out, n),
        Inline::RawInline(n) => mkdocs::raw_inline(out, n),
        _ => false,
    }
}

/// Critic markup: `{++ins++}`, `{--del--}`, `{~~old~>new~~}` and
/// `{>>note<<}` (spec Appendix "PyMdownX compatibility profile"). The
/// highlight `{==x==}` is a plain `Highlight` and prints as its role.
fn critic(out: &mut Out, kind: tmark_ir::Critic<'_>, ctx: Context) {
    let ctx = Context {
        block_start: false,
        ..ctx
    };
    let mut wrap = |open: &str, content: &[Inline], close: &str| {
        out.push(open);
        inlines(out, content, ctx);
        out.push(close);
    };
    match kind {
        tmark_ir::Critic::Insert(content) => wrap("{++", content, "++}"),
        tmark_ir::Critic::Delete(content) => wrap("{--", content, "--}"),
        tmark_ir::Critic::Substitute { old, new } => {
            wrap("{~~", old, "~>");
            wrap("", new, "~~}");
        }
        tmark_ir::Critic::Comment(text) => {
            out.push("{>>");
            out.push(text);
            out.push("<<}");
        }
    }
}

/// The shortcode of an icon span: exactly `.icon media=web` around one
/// `Str` of the form `:material-…:` (one of the four Material sets).
pub fn icon_shortcode(n: &tmark_ir::SpanNode) -> Option<&str> {
    let [Inline::Str(s)] = n.content.as_slice() else {
        return None;
    };
    let name = s.text.strip_prefix(':')?.strip_suffix(':')?;
    let well_formed = !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'+')
        });
    let attrs = &n.attrs;
    let icon_only = attrs.id.is_none()
        && attrs.classes == ["icon"]
        && attrs.kv == [("media".to_string(), "web".to_string())];
    (well_formed && icon_only && tmark_ir::emoji::is_icon(name)).then_some(s.text.as_str())
}

/// The inlines of a paragraph-like block, for content that must print on one
/// line; `None` for blocks whose content is not inline.
fn para_like(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Para(p) => Some(&p.content),
        Block::Plain(p) => Some(&p.content),
        _ => None,
    }
}

pub fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
        Side::Outer => "outer",
        Side::Inner => "inner",
    }
}

/// Text inside a role's brackets that is not Markdown (code): brackets and
/// backslashes escaped.
fn group_verbatim(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

/// A code span with a backtick run longer than any inside, padded when the
/// text starts or ends with a backtick. In a pipe-table cell a `|` is
/// written `\|` (GFM splits cells on it even inside code, and decodes the
/// escape).
pub fn code_span(out: &mut Out, text: &str, in_cell: bool) {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty();
    out.push(&fence);
    if pad {
        out.push(" ");
    }
    if in_cell {
        out.push(&text.replace('|', "\\|"));
    } else {
        out.push(text);
    }
    if pad {
        out.push(" ");
    }
    out.push(&fence);
}

fn link(out: &mut Out, n: &tmark_ir::Link, ctx: Context) {
    // Autolink literals print bare when the text is the address.
    if let Target::Url(url) = &n.target {
        if let [Inline::Str(s)] = n.content.as_slice() {
            let bare = (url == &s.text
                && (url.starts_with("http://") || url.starts_with("https://")))
                || (s.text.starts_with("www.") && *url == format!("http://{}", s.text));
            let mail = url.strip_prefix("mailto:") == Some(s.text.as_str());
            if bare || mail {
                out.push(&s.text);
                return;
            }
        }
    }
    out.push("[");
    inlines(
        out,
        &n.content,
        Context {
            in_group: true,
            block_start: false,
            ..ctx
        },
    );
    // The reference-style form is a spelling of its own: `[text](#id)`
    // points inside the page, `[text][id]` names a label wherever it
    // lives (spec §Ref).
    if let Target::Reference(id) = &n.target {
        out.push("][");
        out.push(id);
        out.push("]");
        return;
    }
    out.push("](");
    match &n.target {
        Target::Url(u) | Target::Document(u) => out.push(&destination(u)),
        Target::Anchor(a) => out.push(&destination(&format!("#{a}"))),
        Target::Reference(_) => unreachable!("written above"),
    }
    if let Some(title) = &n.title {
        out.push(" \"");
        out.push(&title.replace('\\', "\\\\").replace('"', "\\\""));
        out.push("\"");
    }
    out.push(")");
}

/// A link destination as CommonMark reads it back: bare when it is one
/// run of non-space characters with balanced parentheses, else `<…>` with
/// `<`, `>` (and a line break) escaped.
pub fn destination(u: &str) -> String {
    let mut depth = 0i32;
    let balanced = u.chars().all(|c| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        depth >= 0
    }) && depth == 0;
    let simple = !u.is_empty()
        && balanced
        && !u.contains(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
        && !u.starts_with('<');
    if simple {
        u.to_string()
    } else {
        format!(
            "<{}>",
            u.replace('\\', "\\\\")
                .replace('<', "\\<")
                .replace('>', "\\>")
                .replace('\n', "%0A")
        )
    }
}

/// `@key` when one plain item written bare; `@[…]` otherwise (spec §Ref:
/// a lone `@[key]` keeps its brackets, which carry the parenthetical
/// meaning of a citation under `citations.narrative`, C51).
/// A key the bare `@key` grammar accepts (spec §Lexical grammar): a letter,
/// then `[\w:.-]`, ending on an alphanumeric; `doi:` keys and URLs may hold
/// `/`. Anything else (a digit-initial Zotero key, C27) prints bracketed,
/// where the item grammar's fallback keeps the whole item as the key.
fn is_bare_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    let url = key.starts_with("doi:") || key.starts_with("http://") || key.starts_with("https://");
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(b, b'_' | b':' | b'.' | b'-')
                || (url && !b.is_ascii_whitespace() && !matches!(b, b'[' | b']' | b'(' | b')'))
        })
}

/// The X4 guard (spec §Lexical grammar): `@` fires only after a non-word
/// character that is not one of `@/:.-`, so a reference printed right after
/// such a character (`text.` then `[^key]`) needs a space before its sigil.
pub fn blocks_sigil(prev: char) -> bool {
    prev.is_alphanumeric() || matches!(prev, '_' | '@' | '/' | ':' | '.' | '-')
}

fn reference(out: &mut Out, items: &[RefItem], bracketed: bool) {
    if out.last_char().is_some_and(blocks_sigil) {
        out.push(" ");
    }
    if let [item] = items {
        if !bracketed
            && item.prefix.is_none()
            && item.suffix.is_none()
            && !item.suppress_author
            && !item.narrative
            && is_bare_key(&item.key)
        {
            out.push("@");
            out.push(&item.key);
            return;
        }
    }
    out.push("@[");
    let parts: Vec<String> = items
        .iter()
        .map(|item| {
            let mut s = String::new();
            if let Some(prefix) = &item.prefix {
                s.push_str(prefix);
                s.push(' ');
            }
            if item.suppress_author {
                s.push('-');
            } else if item.narrative {
                s.push('+');
            }
            s.push_str(&item.key);
            if let Some(suffix) = &item.suffix {
                s.push_str(", ");
                s.push_str(suffix);
            }
            s
        })
        .collect();
    out.push(&parts.join("; "));
    out.push("]");
}
