//! Inline nodes (writers-and-passes.md §2: escaping, links, footnotes,
//! citations, index, glossary, counters, asides, keystrokes; contract
//! names from fragment-contracts.md §1).

use tmark_ir::{
    plain_text, Abbr, Aside, Image, IndexEntry, Inline, Keystroke, Link, Note, Ref, SpanNode,
    Target,
};
use tmark_registry::Resolution;

use super::escape;
use super::Latex;
use crate::common::{abbr, logos, media, refs, text, zero};
use crate::{CodeEngine, Media};

/// Zero-width inlines (spec §Attributes): comments, index entries, asides,
/// anchor-only spans, raw text of another format.
pub(crate) fn is_zero_width(inline: &Inline) -> bool {
    match inline {
        Inline::Comment(_) | Inline::IndexEntry(_) | Inline::Aside(_) => true,
        Inline::Span(s) => s.content.is_empty() && s.attrs.id.is_some(),
        Inline::RawInline(r) => r.format != "latex",
        Inline::Var(_) => true,
        _ => false,
    }
}

impl Latex<'_> {
    pub(crate) fn inlines(&mut self, inlines: &[Inline]) {
        let zero = |i: &Inline| is_zero_width(i) || media::inline_skipped(i, Media::Print);
        let collapsed = zero::collapse(inlines, &zero);
        for inline in &collapsed {
            if media::inline_skipped(inline, Media::Print) {
                continue;
            }
            self.inline(inline);
        }
    }

    fn wrap(&mut self, command: &str, content: &[Inline]) {
        self.out.push("\\");
        self.out.push(command);
        self.out.push("{");
        self.inlines(content);
        self.out.push("}");
    }

    pub(crate) fn inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Str(s) => {
                let keys = std::mem::take(&mut self.abbr_keys);
                for segment in abbr::split(&s.text, &keys) {
                    match segment {
                        abbr::Segment::Text(t) => self.prose(t),
                        abbr::Segment::Abbr(key) => self.abbr(&Abbr {
                            meta: s.meta,
                            text: key.to_string(),
                        }),
                    }
                }
                self.abbr_keys = keys;
            }
            Inline::Space(_) => self.out.push(" "),
            Inline::SoftBreak(_) => self.out.push("\n"),
            Inline::LineBreak(_) => {
                if self.in_cell {
                    self.out.push("\\newline ");
                } else {
                    self.out.push("\\\\\n");
                }
            }
            Inline::Emph(n) => self.wrap("emph", &n.content),
            Inline::Strong(n) => self.wrap("textbf", &n.content),
            Inline::Strikeout(n) => {
                self.req.package("ulem");
                self.wrap("sout", &n.content);
            }
            Inline::Underline(n) => {
                self.req.package("ulem");
                self.wrap("uline", &n.content);
            }
            Inline::Highlight(n) => {
                self.req.fragment("ts-typesetting");
                self.wrap("tsmark", &n.content);
            }
            Inline::Subscript(n) => self.wrap("textsubscript", &n.content),
            Inline::Superscript(n) => self.wrap("textsuperscript", &n.content),
            Inline::SmallCaps(n) => self.wrap("textsc", &n.content),
            Inline::Quoted(n) => {
                self.req.package("csquotes");
                self.wrap("enquote", &n.content);
            }
            Inline::Code(n) => self.code(n.lang.as_deref(), &n.text),
            Inline::Math(n) => {
                if n.display {
                    self.out.push("\\[");
                    self.out.push(&n.text);
                    self.out.push("\\]");
                } else {
                    self.out.push("$");
                    self.out.push(&n.text);
                    self.out.push("$");
                }
            }
            Inline::Link(n) => self.link(n),
            Inline::Ref(n) => self.reference(n),
            Inline::Note(n) => self.note(n),
            Inline::Image(n) => self.image_inline(n),
            Inline::IndexEntry(n) => self.index_entry(n),
            Inline::CounterItem(n) => {
                let id = format!("{}:{}", n.prefix, n.key);
                let number = refs::number(self.res, &n.prefix, &n.key)
                    .unwrap_or_else(|| refs::unresolved(&id));
                self.req.counters.insert(n.prefix.to_ascii_lowercase());
                self.out.begin(n.meta.id);
                self.out.push(&format!(
                    "\\leavevmode\\phantomsection\\label{{{}}}{}",
                    escape::escape(&id),
                    escape::prose(&number)
                ));
                self.out.end(n.meta.id);
            }
            Inline::Keystroke(n) => self.keystroke(n),
            Inline::Aside(n) => self.aside(n),
            Inline::Span(n) => self.span(n),
            Inline::Var(_) => {}
            Inline::Abbr(n) => self.abbr(n),
            Inline::Comment(_) => {}
            Inline::RawInline(n) => {
                if n.format == "latex" {
                    self.out.push(&n.text);
                }
            }
            Inline::ProgressBar(n) => self.progress_bar(n),
        }
    }

    /// Prose with the TeX logo words set as logos under
    /// `typography.tex-logos` (spec §TeX logos): `\TeX{}`, `\LaTeX{}` and
    /// `\LaTeXe{}` are the kernel's, the others `\tslogo{Name}` of
    /// `ts-typesetting`.
    fn prose(&mut self, text: &str) {
        // A hard line break is `\\`, which scans for an optional
        // `[⟨dimen⟩]` across the line end: a text run that opens with `[`
        // right after one would be read as that argument. An empty group
        // stops the scan; outside a table nothing here has to be the
        // first token of its cell, so the guard goes on the command side
        // (`escape::guard_bracket` does the cell side).
        if text.starts_with('[') && self.out.ends_with("\\\\") {
            self.out.push("{}");
        }
        if !self.tex_logos {
            self.out.push(&escape::prose(text));
            return;
        }
        for segment in logos::split(text) {
            match segment {
                logos::Segment::Text(t) => self.out.push(&escape::prose(t)),
                logos::Segment::Logo(name) => match name {
                    "TeX" => self.out.push("\\TeX{}"),
                    "LaTeX" => self.out.push("\\LaTeX{}"),
                    "LaTeX2e" => self.out.push("\\LaTeXe{}"),
                    other => {
                        self.req.fragment("ts-typesetting");
                        self.out.push(&format!("\\tslogo{{{other}}}"));
                    }
                },
            }
        }
    }

    /// `\tsprogress[thin]{0.45}{label}` (`ts-typesetting`, spec
    /// §ProgressBar): the value as a fraction, the label as prose, the
    /// classes as the option (`thin` halves the height; the others are the
    /// web stylesheet's and are forwarded for the template).
    fn progress_bar(&mut self, n: &tmark_ir::ProgressBar) {
        self.req.fragment("ts-typesetting");
        let value = n.value.clamp(0.0, 100.0) / 100.0;
        let label = n
            .label
            .clone()
            .unwrap_or_else(|| format!("{}%", n.value_text()));
        self.out.push("\\tsprogress");
        if !n.attrs.classes.is_empty() {
            self.out.push(&format!("[{}]", n.attrs.classes.join(",")));
        }
        self.out.push(&format!(
            "{{{}}}{{{}}}",
            text::trim_float(value),
            escape::prose(&label)
        ));
    }

    /// `\tscodeinline[lang=py]{…}` (fragment-contracts.md §5) with an
    /// `\allowbreak{}` after each `inline_breaks` character
    /// (`formatter.py:150-170`); a language is dropped under
    /// `inline_plain`.
    fn code(&mut self, lang: Option<&str>, code: &str) {
        self.req.fragment("ts-code");
        let lang = if self.opts.code.inline_plain {
            None
        } else {
            lang
        };
        if lang.is_some() && self.opts.code.engine == CodeEngine::Minted {
            self.req.shell_escape = true;
        }
        self.out.push("\\tscodeinline");
        if let Some(lang) = lang {
            self.out.push(&format!("[lang={lang}]"));
        }
        self.out.push("{");
        self.out
            .push(&inline_code(code, &self.opts.code.inline_breaks));
        self.out.push("}");
    }

    /// `\href` for URLs (`\url` for an autolink), `\hyperref`/`\ref` for
    /// anchors through the textual template, plain text for a `.md` target
    /// the links pass did not rewrite.
    fn link(&mut self, n: &Link) {
        self.out.begin(n.meta.id);
        match &n.target {
            Target::Url(url) => {
                let plain = plain_text(&n.content);
                if plain == *url || n.content.is_empty() {
                    self.out.push(&format!("\\url{{{}}}", escape::url(url)));
                } else {
                    self.out.push(&format!("\\href{{{}}}{{", escape::url(url)));
                    self.inlines(&n.content);
                    self.out.push("}");
                }
            }
            Target::Anchor(id) => self.anchor(n, id),
            // Spec §Ref: the reference-style form is a textual reference
            // to the label it names, and the brackets CommonMark reads
            // when it names none.
            Target::Reference(key) => {
                if refs::refers(self.res, n.meta.id, key) {
                    self.anchor(n, key);
                } else {
                    self.out.push("[");
                    self.inlines(&n.content);
                    self.out.push(&format!("][{}]", escape::prose(key)));
                }
            }
            Target::Document(_) => self.inlines(&n.content),
        }
        self.out.end(n.meta.id);
    }

    /// A link to an anchor of the document: `\ref` when it carries no
    /// text of its own, else `\hyperref` through the textual template.
    fn anchor(&mut self, n: &Link, key: &str) {
        let id = escape::escape(key);
        if n.content.is_empty() {
            self.out.push(&format!("\\ref{{{id}}}"));
            return;
        }
        let text = self.render_inlines(&n.content);
        let rendered = refs::textual(
            &self.opts.refs.textual_print,
            &format!("\\hyperref[{id}]{{{text}}}"),
            &format!("\\ref{{{id}}}"),
            &format!("\\pageref{{{id}}}"),
        );
        self.out.push(&rendered);
    }

    /// `Ref`: labels through `\hyperref`/`\ref`, citations through
    /// `\cite` — or `\textcite` for a `+key` item and for a bare `@key`
    /// under `citations.narrative` (spec §Cite) — glossary terms through
    /// `\tsgls`, `[?key]` when unresolved.
    fn reference(&mut self, n: &Ref) {
        self.out.begin(n.meta.id);
        // Consecutive plain citations of one form merge into one
        // `\cite{k1,k2}`.
        let mut pending: Vec<String> = Vec::new();
        let mut pending_narrative = false;
        let mut written = 0usize;
        let bare_narrative = self.narrative && !n.bracketed;
        let narrative_of = |item: &tmark_ir::RefItem| item.narrative || bare_narrative;
        let flush =
            |this: &mut Self, pending: &mut Vec<String>, written: &mut usize, narrative: bool| {
                if pending.is_empty() {
                    return;
                }
                if *written > 0 {
                    this.out.push(if n.bracketed { "; " } else { ", " });
                }
                *written += 1;
                let command = if narrative { "textcite" } else { "cite" };
                if narrative {
                    this.req.fragment("ts-bibliography");
                }
                this.out
                    .push(&format!("\\{command}{{{}}}", pending.join(",")));
                pending.clear();
            };
        for item in &n.items {
            let resolution = refs::lookup(self.res, n.meta.id, &item.key)
                .map(|r| r.resolution.clone())
                .unwrap_or(Resolution::Unresolved);
            if let Resolution::Citation { key } = &resolution {
                self.req.cite(key);
                let narrative = narrative_of(item);
                if item.prefix.is_none() && item.suffix.is_none() && !item.suppress_author {
                    if pending_narrative != narrative {
                        flush(self, &mut pending, &mut written, pending_narrative);
                    }
                    pending_narrative = narrative;
                    pending.push(key.clone());
                    continue;
                }
                flush(self, &mut pending, &mut written, pending_narrative);
                if written > 0 {
                    self.out.push(if n.bracketed { "; " } else { ", " });
                }
                written += 1;
                let command = if item.suppress_author {
                    "citeyear"
                } else if narrative {
                    "textcite"
                } else {
                    "cite"
                };
                if narrative && !item.suppress_author {
                    self.req.fragment("ts-bibliography");
                }
                self.out.push(&format!("\\{command}"));
                match (&item.prefix, &item.suffix) {
                    (Some(pre), Some(post)) => self.out.push(&format!(
                        "[{}][{}]",
                        escape::prose(pre),
                        escape::prose(post)
                    )),
                    (None, Some(post)) => self.out.push(&format!("[{}]", escape::prose(post))),
                    (Some(pre), None) => self.out.push(&format!("[{}][]", escape::prose(pre))),
                    (None, None) => {}
                }
                self.out.push(&format!("{{{key}}}"));
                continue;
            }
            flush(self, &mut pending, &mut written, pending_narrative);
            if written > 0 {
                self.out.push(if n.bracketed { "; " } else { ", " });
            }
            written += 1;
            if let Some(prefix) = &item.prefix {
                self.out.push(&escape::prose(prefix));
                self.out.push(" ");
            }
            match resolution {
                Resolution::Label { prefix, number, .. } => {
                    // The label as defined: TMark matches keys
                    // case-insensitively, LaTeX does not.
                    let defined = self
                        .res
                        .labels
                        .get(&item.key)
                        .map(|l| l.id.clone())
                        .unwrap_or_else(|| item.key.clone());
                    let key = escape::escape(&defined);
                    // An anchor with no number shows its text (spec
                    // §Anchor, `ref-unnumbered`): a `\\ref` would print the
                    // enclosing section's number.
                    let unnumbered = refs::unnumbered_text(self.res, &item.key);
                    match (&prefix, number, unnumbered) {
                        (_, _, Some(text)) => {
                            self.out
                                .push(&format!("\\hyperref[{key}]{{{}}}", escape::prose(&text)));
                        }
                        (Some(p), Some(number), None) => {
                            // TMark numbers the series: the template as text.
                            let template = refs::reference_template(self.res, p)
                                .replace("{name} {number}", "{name}~{number}");
                            let word =
                                refs::label_word(self.res, p, &item.key, self.lang.as_deref());
                            let label = refs::template(
                                &template,
                                &escape::prose(&word),
                                &escape::prose(&number),
                            );
                            self.out
                                .push(&format!("\\hyperref[{key}]{{{}}}", label.trim()));
                        }
                        (Some(p), None, None) => {
                            let template = refs::reference_template(self.res, p)
                                .replace("{name} {number}", "{name}~{number}");
                            let word =
                                refs::label_word(self.res, p, &item.key, self.lang.as_deref());
                            let label = refs::template(
                                &template,
                                &escape::prose(&word),
                                &format!("\\ref{{{key}}}"),
                            );
                            self.out.push(label.trim());
                        }
                        (None, _, None) => self.out.push(&format!("\\ref{{{key}}}")),
                    }
                }
                Resolution::Glossary { term } => {
                    self.req.fragment("ts-glossary");
                    self.req.package("glossaries");
                    self.out
                        .push(&format!("\\tsgls{{{}}}", escape::escape(&term)));
                }
                Resolution::Doi { doi } => {
                    self.out.push(&format!(
                        "\\href{{https://doi.org/{}}}{{{}}}",
                        escape::url(&doi),
                        escape::prose(&doi)
                    ));
                }
                // A sibling document is outside this build: its label as
                // text, no `\ref` to a label LaTeX never sees.
                Resolution::Sibling { label, .. } => self.out.push(&escape::prose(&label)),
                Resolution::External { label, page, .. } => {
                    self.out.push(&escape::prose(&label));
                    if let Some(page) = page {
                        self.out.push(&format!(" p.~{page}"));
                    }
                }
                Resolution::Citation { .. } => unreachable!("handled above"),
                Resolution::Ambiguous | Resolution::Unresolved => {
                    self.out.push(&escape::prose(&refs::unresolved(&item.key)));
                }
            }
            if let Some(suffix) = &item.suffix {
                self.out.push(", ");
                self.out.push(&escape::prose(suffix));
            }
        }
        flush(self, &mut pending, &mut written, pending_narrative);
        self.out.end(n.meta.id);
    }

    /// `\footnote{body}`; a definition is looked up by label, an inline
    /// note carries its body. Several paragraphs are joined by `\par`.
    fn note(&mut self, n: &Note) {
        let body = match &n.label {
            Some(label) => {
                let content = self
                    .doc
                    .footnotes
                    .iter()
                    .find(|f| f.label == *label)
                    .map(|f| f.content.clone());
                match content {
                    Some(content) => self.render_blocks_inline(&content),
                    None => escape::prose(&refs::unresolved(&format!("^{label}"))),
                }
            }
            None => self.render_blocks_inline(&n.content),
        };
        self.out.begin(n.meta.id);
        self.out.push(&format!("\\footnote{{{body}}}"));
        self.out.end(n.meta.id);
    }

    /// An image in running text: `\includegraphics` without a float; an
    /// `Image{.icon}` (the emoji pass in artifact mode) is `\tsicon{path}`
    /// (fragment-contracts.md §1, row `icon`).
    fn image_inline(&mut self, n: &Image) {
        if n.src.is_empty() {
            // A generated image the assets pass did not render.
            return;
        }
        self.req.assets.push(crate::AssetRef {
            src: n.src.clone(),
            node: n.meta.id,
            attrs: n.attrs.kv.clone(),
        });
        if n.attrs.has_class("icon") {
            self.req.fragment("ts-typesetting");
            self.out.begin(n.meta.id);
            self.out.push(&format!(
                "\\tsicon{{{}}}",
                escape::escape(super::figure::strip_theme_variant(&n.src))
            ));
            self.out.end(n.meta.id);
            return;
        }
        self.req.package("graphicx");
        self.out.begin(n.meta.id);
        self.out.push(&format!(
            "\\includegraphics[width={}]{{{}}}",
            super::figure::width(n.attrs.get("width"), "1em"),
            escape::escape(super::figure::strip_theme_variant(&n.src))
        ));
        self.out.end(n.meta.id);
    }

    /// `\tsindex[registry=r, main]{sort@formatted!sub}` (`index.tex`,
    /// `extensions/index/renderer.py`): the sort key is the plain text,
    /// emitted only when the formatted entry differs.
    fn index_entry(&mut self, n: &IndexEntry) {
        self.req.fragment("ts-index");
        self.req.package("imakeidx");
        self.req
            .index
            .insert(n.registry.clone().unwrap_or_default());
        let mut levels: Vec<String> = Vec::new();
        for level in &n.path {
            let formatted = self.render_inlines(level);
            let sort = escape::prose(&plain_text(level));
            if formatted == sort {
                levels.push(formatted);
            } else {
                levels.push(format!("{sort}@{formatted}"));
            }
        }
        let mut keys: Vec<String> = Vec::new();
        if let Some(registry) = &n.registry {
            keys.push(format!("registry={registry}"));
        }
        if n.main {
            keys.push("main".to_string());
        }
        self.out.push("\\tsindex");
        if !keys.is_empty() {
            self.out.push(&format!("[{}]", keys.join(", ")));
        }
        self.out.push(&format!("{{{}}}", levels.join("!")));
    }

    /// `\tskeys{Ctrl,Alt,Del}` with the label table; a literal comma is
    /// braced (fragment-contracts.md §1).
    fn keystroke(&mut self, n: &Keystroke) {
        self.req.fragment("ts-keystrokes");
        let labels: Vec<String> = n
            .keys
            .iter()
            .map(|k| {
                let label = text::key_label(k);
                match label.as_str() {
                    "," => "{,}".to_string(),
                    "↑" => "\\(\\uparrow\\)".to_string(),
                    "↓" => "\\(\\downarrow\\)".to_string(),
                    _ => escape::prose(&label),
                }
            })
            .collect();
        self.out.push(&format!("\\tskeys{{{}}}", labels.join(",")));
    }

    /// `\tsaside[side=left]{…}`.
    fn aside(&mut self, n: &Aside) {
        self.req.fragment("ts-typesetting");
        let body = self.render_blocks_inline(&n.content);
        if body.is_empty() {
            return;
        }
        self.out.push("\\tsaside");
        if let Some(side) = n.side {
            self.out.push(&format!("[side={}]", side_name(side)));
        }
        self.out.push(&format!("{{{body}}}"));
    }

    /// `Span`: an anchor (`\phantomsection\label`), a language switch
    /// (`\foreignlanguage`), a script run (`\tsscript`), an emoji
    /// (`\tsemoji`), else transparent.
    /// Critic markup through the `ts-critic` contract
    /// (fragment-contracts.md §1): `\tsins{…}`, `\tsdel{…}`,
    /// `\tssubst{old}{new}`, `\tscomment{…}`. A critic comment is a
    /// reviewer's annotation, not an author's `<!-- … -->`: it is typeset,
    /// which is the whole point of building a review PDF.
    fn critic(&mut self, kind: tmark_ir::Critic<'_>) {
        self.req.fragment("ts-critic");
        match kind {
            tmark_ir::Critic::Insert(content) => self.wrap("tsins", content),
            tmark_ir::Critic::Delete(content) => self.wrap("tsdel", content),
            tmark_ir::Critic::Substitute { old, new } => {
                self.out.push("\\tssubst{");
                self.inlines(old);
                self.out.push("}{");
                self.inlines(new);
                self.out.push("}");
            }
            tmark_ir::Critic::Comment(text) => {
                self.out.push("\\tscomment{");
                self.out.push(&escape::prose(text));
                self.out.push("}");
            }
        }
    }

    fn span(&mut self, n: &SpanNode) {
        if let Some(kind) = tmark_ir::critic(n) {
            self.critic(kind);
            return;
        }
        if let Some(id) = n.attrs.id() {
            self.out.push(&format!(
                "\\phantomsection\\label{{{}}}",
                escape::escape(id)
            ));
        }
        if let Some(slug) = n.attrs.get("script") {
            self.req.fragment("ts-fonts");
            self.out.push(&format!(
                "\\tsscript{{{}}}{{{}}}",
                escape::escape(slug),
                escape::prose(&plain_text(&n.content))
            ));
            return;
        }
        if n.attrs.get("emoji").is_some() {
            self.req.fragment("ts-fonts");
            self.out.push("\\tsemoji{");
            self.out.push(&plain_text(&n.content));
            self.out.push("}");
            return;
        }
        match n.attrs.lang() {
            Some(lang) if self.opts.lang.as_deref() != Some(lang) => {
                self.req.package("babel");
                self.out
                    .push(&format!("\\foreignlanguage{{{}}}{{", babel_name(lang)));
                self.inlines(&n.content);
                self.out.push("}");
            }
            _ => self.inlines(&n.content),
        }
    }

    /// `Abbr` → `\tsacr{key}` when the term has an expansion, else the
    /// escaped text (`writer.py:788-798`). The key is
    /// `text::acronym_key` with `2`, `3`, … on a collision; `Requires.
    /// acronyms` lists the keys so `ts-glossary` declares exactly those.
    fn abbr(&mut self, n: &Abbr) {
        let term = n.text.trim();
        let expansion = self
            .doc
            .abbreviations
            .iter()
            .find(|a| a.key == term)
            .map(|a| a.expansion.clone())
            .or_else(|| self.res.glossary.get(&term.to_ascii_lowercase()).cloned());
        let Some(expansion) = expansion.filter(|e| !e.trim().is_empty()) else {
            self.out.push(&escape::prose(term));
            return;
        };
        let _ = expansion;
        let key = match self.acronyms.get(term) {
            Some(key) => key.clone(),
            None => {
                let base = text::acronym_key(term);
                let mut candidate = base.clone();
                let mut suffix = 2;
                while self.acronyms.values().any(|k| *k == candidate) {
                    candidate = format!("{base}{suffix}");
                    suffix += 1;
                }
                self.acronyms.insert(term.to_string(), candidate.clone());
                candidate
            }
        };
        self.req.fragment("ts-glossary");
        self.req.package("glossaries");
        self.req.acronym(&key);
        self.out.push(&format!("\\tsacr{{{key}}}"));
    }
}

/// Escapes inline code and inserts `\allowbreak{}` after each break
/// character (`formatter.py:150-170`): the text is split before escaping
/// so the macros the escaper produces stay out of reach.
pub(crate) fn inline_code(code: &str, breaks: &str) -> String {
    if breaks.is_empty() {
        return escape::escape(code);
    }
    let mut out = String::with_capacity(code.len() + 8);
    let mut piece = String::new();
    for c in code.chars() {
        if breaks.contains(c) {
            out.push_str(&escape::escape(&piece));
            piece.clear();
            out.push_str(&escape::escape(&c.to_string()));
            out.push_str("\\allowbreak{}");
        } else {
            piece.push(c);
        }
    }
    out.push_str(&escape::escape(&piece));
    out
}

fn side_name(side: tmark_ir::Side) -> &'static str {
    match side {
        tmark_ir::Side::Left => "left",
        tmark_ir::Side::Right => "right",
        tmark_ir::Side::Outer => "outer",
        tmark_ir::Side::Inner => "inner",
    }
}

/// The babel name of a BCP 47 tag, for `\foreignlanguage`.
fn babel_name(lang: &str) -> &str {
    let base = lang.split(['-', '_']).next().unwrap_or(lang);
    match base.to_ascii_lowercase().as_str() {
        "en" => "english",
        "fr" => "french",
        "de" => "german",
        "it" => "italian",
        "es" => "spanish",
        "pt" => "portuguese",
        "nl" => "dutch",
        "la" => "latin",
        "el" => "greek",
        "ru" => "russian",
        _ => lang,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_code_breaks() {
        assert_eq!(
            inline_code("pymdown-extensions", "-"),
            "pymdown-\\allowbreak{}extensions"
        );
        assert_eq!(inline_code("a_b", "-"), "a\\_b");
        assert_eq!(inline_code("a_b", "_"), "a\\_\\allowbreak{}b");
        assert_eq!(inline_code("x", ""), "x");
    }

    #[test]
    fn babel() {
        assert_eq!(babel_name("fr-CH"), "french");
        assert_eq!(babel_name("xx"), "xx");
    }
}
