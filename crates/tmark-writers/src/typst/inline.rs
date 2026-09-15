//! Inline nodes for Typst (`texsmith.typ` names, fragment-contracts.md §1).

use tmark_ir::{
    plain_text, Abbr, Aside, Image, IndexEntry, Inline, Keystroke, Link, Note, QuoteKind, Ref,
    SpanNode, Target,
};
use tmark_registry::Resolution;

use super::escape;
use super::math;
use super::Typst;
use crate::common::{abbr, logos, media, refs, text, zero};
use crate::Media;

/// Zero-width inlines (spec §Attributes).
pub(crate) fn is_zero_width(inline: &Inline) -> bool {
    match inline {
        Inline::Comment(_) | Inline::IndexEntry(_) | Inline::Aside(_) | Inline::Var(_) => true,
        Inline::Span(s) => s.content.is_empty() && s.attrs.id.is_some(),
        Inline::RawInline(r) => r.format != "typst",
        _ => false,
    }
}

impl Typst<'_> {
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

    fn call(&mut self, function: &str, content: &[Inline]) {
        self.out.push(&format!("#{function}["));
        self.inlines(content);
        self.out.push("]");
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
            Inline::LineBreak(_) => self.out.push(" \\\n"),
            Inline::Emph(n) => {
                self.out.push("_");
                self.inlines(&n.content);
                self.out.push("_");
            }
            Inline::Strong(n) => {
                self.out.push("*");
                self.inlines(&n.content);
                self.out.push("*");
            }
            Inline::Strikeout(n) => self.call("strike", &n.content),
            Inline::Underline(n) => self.call("underline", &n.content),
            Inline::Highlight(n) => self.call("highlight", &n.content),
            Inline::Subscript(n) => self.call("sub", &n.content),
            Inline::Superscript(n) => self.call("super", &n.content),
            Inline::SmallCaps(n) => self.call("smallcaps", &n.content),
            Inline::Quoted(n) => {
                let q = match n.kind {
                    QuoteKind::Double => "\"",
                    QuoteKind::Single => "'",
                };
                self.out.push(q);
                self.inlines(&n.content);
                self.out.push(q);
            }
            Inline::Code(n) => {
                let lang = if self.opts.code.inline_plain {
                    None
                } else {
                    n.lang.as_deref()
                };
                match lang {
                    Some(lang) => self.out.push(&format!(
                        "#raw(lang: \"{}\", \"{}\")",
                        escape::string(lang),
                        escape::string(&n.text)
                    )),
                    None => self.out.push(&escape::raw_inline(&n.text)),
                }
            }
            Inline::Math(n) => {
                let rendered = math::render(&n.text, n.display, None);
                if rendered.mitex {
                    self.req.package(math::MITEX_PACKAGE);
                }
                if rendered.labelled {
                    self.req.fragment("ts-equations");
                }
                self.out.push(&rendered.text);
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
                    "{}<{}>",
                    escape::markup(&number),
                    escape::label(&id)
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
                if n.format == "typst" {
                    self.out.push(&n.text);
                }
            }
            Inline::ProgressBar(n) => self.progress_bar(n),
        }
    }

    /// Prose with the TeX logo words as `#ts-logo("Name")` under
    /// `typography.tex-logos` (spec §TeX logos).
    fn prose(&mut self, text: &str) {
        if !self.tex_logos {
            self.out.push(&escape::markup(text));
            return;
        }
        for segment in logos::split(text) {
            match segment {
                logos::Segment::Text(t) => self.out.push(&escape::markup(t)),
                logos::Segment::Logo(name) => {
                    self.req.fragment("ts-typesetting");
                    self.out.push(&format!("#ts-logo(\"{name}\")"));
                }
            }
        }
    }

    /// `#ts-progress(0.45, label: "…", thin: true)` (spec §ProgressBar):
    /// the value as a fraction; `thin` when the class is set, the other
    /// classes as `class: ("a", "b")`.
    fn progress_bar(&mut self, n: &tmark_ir::ProgressBar) {
        self.req.fragment("ts-typesetting");
        let value = n.value.clamp(0.0, 100.0) / 100.0;
        let mut args = vec![crate::common::text::trim_float(value)];
        let label = n
            .label
            .clone()
            .unwrap_or_else(|| format!("{}%", n.value_text()));
        args.push(format!("label: \"{}\"", escape::string(&label)));
        if n.attrs.has_class("thin") {
            args.push("thin: true".to_string());
        }
        let classes: Vec<String> = n
            .attrs
            .classes
            .iter()
            .filter(|c| *c != "thin")
            .map(|c| format!("\"{}\"", escape::string(c)))
            .collect();
        if !classes.is_empty() {
            args.push(format!("class: ({},)", classes.join(", ")));
        }
        self.out.push(&format!("#ts-progress({})", args.join(", ")));
    }

    /// `#link("url")[text]`; an anchor through the textual template with
    /// `#link(<id>)[text]` and `#ref(<id>, supplement: none)`.
    fn link(&mut self, n: &Link) {
        self.out.begin(n.meta.id);
        match &n.target {
            Target::Url(url) => {
                let url = escape::string(url);
                if n.content.is_empty() || plain_text(&n.content) == *url {
                    self.out.push(&format!("#link(\"{url}\")"));
                } else {
                    self.out.push(&format!("#link(\"{url}\")["));
                    self.inlines(&n.content);
                    self.out.push("]");
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
                    self.out.push("\\[");
                    self.inlines(&n.content);
                    self.out.push(&format!("\\]\\[{}\\]", escape::markup(key)));
                }
            }
            Target::Document(_) => self.inlines(&n.content),
        }
        self.out.end(n.meta.id);
    }

    /// A link to an anchor of the document: `#ref` when it carries no
    /// text of its own, else `#link` through the textual template.
    fn anchor(&mut self, n: &Link, key: &str) {
        let label = escape::label(key);
        if n.content.is_empty() {
            self.out.push(&format!("#ref(<{label}>, supplement: none)"));
            return;
        }
        let text = self.render_inlines(&n.content);
        let rendered = refs::textual(
            &self.opts.refs.textual_print,
            &format!("#link(<{label}>)[{text}]"),
            &format!("#ref(<{label}>, supplement: none)"),
            &format!("#ts-page(<{label}>)"),
        );
        self.out.push(&rendered);
    }

    /// `Ref`: `#link(<key>)[Label]` for TMark-numbered series,
    /// `#ref(<key>)` for backend-numbered ones, `#cite(<key>)` for
    /// citations (`form: "prose"` for a `+key` item and for a bare `@key`
    /// under `citations.narrative`, spec §Cite), `#ts-gls("term")`,
    /// `[?key]` when unresolved.
    fn reference(&mut self, n: &Ref) {
        self.out.begin(n.meta.id);
        for (written, item) in n.items.iter().enumerate() {
            let resolution = refs::lookup(self.res, n.meta.id, &item.key)
                .map(|r| r.resolution.clone())
                .unwrap_or(Resolution::Unresolved);
            if written > 0 && !matches!(resolution, Resolution::Citation { .. }) {
                self.out.push(if n.bracketed { "; " } else { ", " });
            }
            if let Some(prefix) = &item.prefix {
                if !matches!(resolution, Resolution::Citation { .. }) {
                    self.out.push(&escape::markup(prefix));
                    self.out.push(" ");
                }
            }
            match resolution {
                Resolution::Label { prefix, number, .. } => {
                    let subfigure = self
                        .res
                        .labels
                        .get(&item.key)
                        .and_then(|l| l.subfigure.clone());
                    let defined = self
                        .res
                        .labels
                        .get(&item.key)
                        .map(|l| l.id.clone())
                        .unwrap_or_else(|| item.key.clone());
                    // Typst carries one label per element: the lone image
                    // of a `::: figure` *is* the container's float, and
                    // the container keeps the label (spec §Image, Figure).
                    let defined = match &subfigure {
                        Some(s) if s.letter.is_none() => s.parent.clone().unwrap_or(defined),
                        _ => defined,
                    };
                    // A lettered sub-figure is numbered in a counter of
                    // its own, so `#ref` alone would show `(a)`, not `2a`.
                    let lettered = subfigure.is_some_and(|s| s.letter.is_some());
                    let label = escape::label(&defined);
                    // An anchor with no number shows its text (spec
                    // §Anchor, `ref-unnumbered`): `#ref` would refuse it.
                    let unnumbered = refs::unnumbered_text(self.res, &item.key);
                    match (&prefix, number, unnumbered) {
                        (_, _, Some(text)) => {
                            self.out
                                .push(&format!("#link(<{label}>)[{}]", escape::markup(&text)));
                        }
                        (Some(p), Some(number), None) => {
                            let template = refs::reference_template(self.res, p);
                            let word =
                                refs::label_word(self.res, p, &item.key, self.lang.as_deref());
                            let text = refs::template(
                                &template,
                                &escape::markup(&word),
                                &escape::markup(&number),
                            );
                            self.out.push(&format!("#link(<{label}>)[{}]", text.trim()));
                        }
                        (Some(p), None, None) if lettered => {
                            let template = refs::reference_template(self.res, p);
                            let word =
                                refs::label_word(self.res, p, &item.key, self.lang.as_deref());
                            let text = refs::template(
                                &template,
                                &escape::markup(&word),
                                &format!("#ts-subnumber(<{label}>)"),
                            );
                            self.out.push(&format!("#link(<{label}>)[{}]", text.trim()));
                        }
                        (Some(p), None, None) => {
                            let word =
                                refs::label_word(self.res, p, &item.key, self.lang.as_deref());
                            if word.is_empty() {
                                self.out.push(&format!("#ref(<{label}>, supplement: none)"));
                            } else {
                                self.out.push(&format!(
                                    "#ref(<{label}>, supplement: [{}])",
                                    escape::markup(&word)
                                ));
                            }
                        }
                        (None, _, None) => {
                            self.out.push(&format!("#ref(<{label}>, supplement: none)"))
                        }
                    }
                }
                Resolution::Citation { key } => {
                    self.req.cite(&key);
                    let mut args = vec![format!("<{}>", escape::label(&key))];
                    if let Some(suffix) = &item.suffix {
                        args.push(format!("supplement: [{}]", escape::markup(suffix)));
                    }
                    if item.suppress_author {
                        args.push("form: \"year\"".to_string());
                    } else if item.narrative || (self.narrative && !n.bracketed) {
                        args.push("form: \"prose\"".to_string());
                    }
                    if let Some(prefix) = &item.prefix {
                        self.out.push(&escape::markup(prefix));
                        self.out.push(" ");
                    }
                    self.out.push(&format!("#cite({})", args.join(", ")));
                    continue;
                }
                Resolution::Glossary { term } => {
                    self.req.fragment("ts-glossary");
                    self.out
                        .push(&format!("#ts-gls(\"{}\")", escape::string(&term)));
                }
                Resolution::Doi { doi } => {
                    self.out.push(&format!(
                        "#link(\"https://doi.org/{}\")[{}]",
                        escape::string(&doi),
                        escape::markup(&doi)
                    ));
                }
                // A sibling document is outside this build: its label as text.
                Resolution::Sibling { label, .. } => self.out.push(&escape::markup(&label)),
                Resolution::External { label, page, .. } => {
                    self.out.push(&escape::markup(&label));
                    if let Some(page) = page {
                        self.out.push(&format!(" p.~{page}"));
                    }
                }
                Resolution::Ambiguous | Resolution::Unresolved => {
                    self.out.push(&escape::markup(&refs::unresolved(&item.key)));
                }
            }
            if let Some(suffix) = &item.suffix {
                self.out.push(", ");
                self.out.push(&escape::markup(suffix));
            }
        }
        self.out.end(n.meta.id);
    }

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
                    None => escape::markup(&refs::unresolved(&format!("^{label}"))),
                }
            }
            None => self.render_blocks_inline(&n.content),
        };
        self.out.begin(n.meta.id);
        self.out.push(&format!("#footnote[{body}]"));
        self.out.end(n.meta.id);
    }

    fn image_inline(&mut self, n: &Image) {
        if n.src.is_empty() {
            return;
        }
        self.req.assets.push(crate::AssetRef {
            src: n.src.clone(),
            node: n.meta.id,
            attrs: n.attrs.kv.clone(),
        });
        self.out.begin(n.meta.id);
        let src = escape::string(crate::latex::figure_src(&n.src));
        if n.attrs.has_class("icon") {
            // `\tsicon{path}`: an inline icon the height of the line.
            self.out
                .push(&format!("#box(image(\"{src}\"), height: 1em)"));
            self.out.end(n.meta.id);
            return;
        }
        let width = n.attrs.get("width").map(|w| {
            if w.ends_with('%') || w.parse::<f64>().is_err() {
                w.to_string()
            } else {
                format!("{w}%")
            }
        });
        match width {
            Some(width) => self
                .out
                .push(&format!("#box(image(\"{src}\", width: {width}))")),
            None => self
                .out
                .push(&format!("#box(image(\"{src}\", height: 1em))")),
        }
        self.out.end(n.meta.id);
    }

    /// `#ts-index("a", "b", registry: "r", main: true)`.
    fn index_entry(&mut self, n: &IndexEntry) {
        self.req.fragment("ts-index");
        self.req
            .index
            .insert(n.registry.clone().unwrap_or_default());
        let mut args: Vec<String> = n
            .path
            .iter()
            .map(|level| format!("[{}]", self.render_inlines(level)))
            .collect();
        if let Some(registry) = &n.registry {
            args.push(format!("registry: \"{}\"", escape::string(registry)));
        }
        if n.main {
            args.push("main: true".to_string());
        }
        self.out.push(&format!("#ts-index({})", args.join(", ")));
    }

    /// `#ts-keys("Ctrl", "S")`.
    fn keystroke(&mut self, n: &Keystroke) {
        self.req.fragment("ts-keystrokes");
        let keys: Vec<String> = n
            .keys
            .iter()
            .map(|k| format!("\"{}\"", escape::string(&text::key_label(k))))
            .collect();
        self.out.push(&format!("#ts-keys({})", keys.join(", ")));
    }

    /// `#ts-aside(side: "left")[…]`.
    fn aside(&mut self, n: &Aside) {
        self.req.fragment("ts-typesetting");
        let body = self.render_blocks_inline(&n.content);
        if body.is_empty() {
            return;
        }
        self.out.push("#ts-aside");
        if let Some(side) = n.side {
            self.out.push(&format!("(side: \"{}\")", side_name(side)));
        }
        self.out.push(&format!("[{body}]"));
    }

    /// A label after the content, `#text(lang: "en")[…]`, `#ts-script`,
    /// `#ts-emoji`.
    /// Critic markup, the Typst side of the `ts-critic` contract
    /// (`assets/texsmith.typ`): `#ts-ins`, `#ts-del`, `#ts-subst`,
    /// `#ts-comment`.
    fn critic(&mut self, kind: tmark_ir::Critic<'_>) {
        self.req.fragment("ts-critic");
        match kind {
            tmark_ir::Critic::Insert(content) => self.call("ts-ins", content),
            tmark_ir::Critic::Delete(content) => self.call("ts-del", content),
            tmark_ir::Critic::Substitute { old, new } => {
                self.out.push("#ts-subst[");
                self.inlines(old);
                self.out.push("][");
                self.inlines(new);
                self.out.push("]");
            }
            tmark_ir::Critic::Comment(text) => {
                self.out.push("#ts-comment[");
                self.out.push(&escape::markup(text));
                self.out.push("]");
            }
        }
    }

    fn span(&mut self, n: &SpanNode) {
        if let Some(kind) = tmark_ir::critic(n) {
            self.critic(kind);
            return;
        }
        let label = n.attrs.id().map(escape::label);
        if let Some(slug) = n.attrs.get("script") {
            self.req.fragment("ts-fonts");
            self.out.push(&format!(
                "#ts-script(\"{}\")[{}]",
                escape::string(slug),
                escape::markup(&plain_text(&n.content))
            ));
        } else if n.attrs.get("emoji").is_some() {
            self.req.fragment("ts-fonts");
            self.out
                .push(&format!("#ts-emoji[{}]", plain_text(&n.content)));
        } else {
            match n.attrs.lang() {
                Some(lang) if self.opts.lang.as_deref() != Some(lang) => {
                    self.out
                        .push(&format!("#text(lang: \"{}\")[", escape::string(lang)));
                    self.inlines(&n.content);
                    self.out.push("]");
                }
                _ => self.inlines(&n.content),
            }
        }
        if let Some(label) = label {
            if n.content.is_empty() {
                // An invisible labelled element: the anchor alone.
                self.out.push(&format!("#metadata(none) <{label}>"));
            } else {
                self.out.push(&format!("<{label}>"));
            }
        }
    }

    /// `#ts-acr("KEY")` with the LaTeX key rule; the escaped text without
    /// an expansion.
    fn abbr(&mut self, n: &Abbr) {
        let term = n.text.trim();
        let expansion = self
            .doc
            .abbreviations
            .iter()
            .find(|a| a.key == term)
            .map(|a| a.expansion.clone())
            .or_else(|| self.res.glossary.get(&term.to_ascii_lowercase()).cloned());
        if !expansion.is_some_and(|e| !e.trim().is_empty()) {
            self.out.push(&escape::markup(term));
            return;
        }
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
        self.req.acronym(&key);
        self.out
            .push(&format!("#ts-acr(\"{}\")", escape::string(&key)));
    }
}

fn side_name(side: tmark_ir::Side) -> &'static str {
    match side {
        tmark_ir::Side::Left => "left",
        tmark_ir::Side::Right => "right",
        tmark_ir::Side::Outer => "outer",
        tmark_ir::Side::Inner => "inner",
    }
}
