//! Figures and images (writers-and-passes.md §2 "Figures, captions,
//! labels"; `figure.tex`, `figure_tcolorbox.tex`): `figure[H]`,
//! `\includegraphics[width=…]`, `\caption[short]{long}` **then** `\label`;
//! inside a box, `center` and `\captionof{figure}`; a `::: figure` with
//! several images becomes sub-figures.

use tmark_ir::{plain_text, Block, Caption, Figure, Image};

use super::escape;
use super::Latex;

/// `width="50%"` → `0.5\linewidth`, `100%` → `\linewidth`, absent →
/// `default`, anything else verbatim.
pub(crate) fn width(attr: Option<&str>, default: &str) -> String {
    match attr {
        None => default.to_string(),
        Some(w) => match w.strip_suffix('%').and_then(|n| n.parse::<f64>().ok()) {
            Some(percent) => percent_linewidth(percent / 100.0),
            None => w.to_string(),
        },
    }
}

/// `0.25\linewidth` with at most four decimals, `\linewidth` for 1.
pub(crate) fn percent_linewidth(factor: f64) -> String {
    if (factor - 1.0).abs() < 1e-9 {
        return "\\linewidth".to_string();
    }
    let mut s = format!("{factor:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    format!("{s}\\linewidth")
}

/// `src#only-light` / `#only-dark` → `src` (`writer.py:1083`).
pub(crate) fn strip_theme_variant(src: &str) -> &str {
    match src.rsplit_once('#') {
        Some((base, variant))
            if variant.eq_ignore_ascii_case("only-light")
                || variant.eq_ignore_ascii_case("only-dark") =>
        {
            base
        }
        _ => src,
    }
}

impl Latex<'_> {
    /// A lone image: a `figure` with the caption line's text, else the alt
    /// text as caption (legacy `render_images`); `label` overrides the
    /// anchor (a `::: figure` id wins over the image's).
    pub(crate) fn figure_image(
        &mut self,
        image: &Image,
        caption: Option<&Caption>,
        link: Option<&str>,
        label: Option<&str>,
    ) {
        if image.src.is_empty() {
            // A generated image (`python image`, `mermaid`) the assets pass
            // did not render: nothing to include.
            return;
        }
        self.req.package("graphicx");
        self.req.assets.push(crate::AssetRef {
            src: image.src.clone(),
            node: image.meta.id,
            attrs: image.attrs.kv.clone(),
        });
        let alt_plain = plain_text(&image.alt).trim().to_string();
        let alt = self.render_inlines(&image.alt);
        let caption_text = match caption {
            Some(c) => Some(self.render_inlines(&c.content)),
            None if !alt.is_empty() => Some(alt.clone()),
            None => None,
        };
        let caption_plain = caption
            .map(|c| plain_text(&c.content))
            .unwrap_or_else(|| alt_plain.clone());
        // The alt is the short caption unless longer than the caption
        // (`writer.py:1112`).
        let short = (!alt_plain.is_empty()
            && caption.is_some()
            && alt_plain.chars().count() <= caption_plain.trim().chars().count())
        .then_some(alt);
        let mut labels: Vec<String> = label
            .map(str::to_string)
            .or_else(|| caption.and_then(|c| c.attrs.id.clone()))
            .or_else(|| image.attrs.id.clone())
            .into_iter()
            .collect();
        // The lone image of a `::: figure` is the figure itself (spec
        // §Image, Figure): its anchor names the same float as the
        // container's, so both `\label`s go on the one caption.
        if let (Some(own), true) = (image.attrs.id.clone(), label.is_some()) {
            if !labels.contains(&own) {
                labels.push(own);
            }
        }
        let include = format!(
            "\\includegraphics[width={}]{{{}}}",
            width(image.attrs.get("width"), "\\linewidth"),
            escape::escape(strip_theme_variant(&image.src))
        );
        // A generated diagram (`generate=<lang>`, converted by the assets
        // pass) may be wider than the text: `media.py:213`.
        let include = if image.attrs.get("generate").is_some() {
            self.req.package("adjustbox");
            format!("\\adjustbox{{max width=\\textwidth}}{{{include}}}")
        } else {
            include
        };
        let include = match link {
            Some(url) => format!("\\href{{{}}}{{{include}}}", escape::url(url)),
            None => include,
        };
        if self.in_box > 0 {
            self.req.package("caption");
            self.out.push("\\begin{center}\n");
            self.out.begin(image.meta.id);
            self.out.push(&include);
            self.out.end(image.meta.id);
            self.out.push("\n");
            self.caption_line(
                "\\captionof{figure}",
                caption_text.as_deref(),
                short.as_deref(),
                &labels,
                caption,
            );
            self.out.push("\\end{center}\n");
        } else {
            self.req.package("float");
            self.out.push("\\begin{figure}[H]\n\\centering\n");
            self.out.begin(image.meta.id);
            self.out.push(&include);
            self.out.end(image.meta.id);
            self.out.push("\n");
            self.caption_line(
                "\\caption",
                caption_text.as_deref(),
                short.as_deref(),
                &labels,
                caption,
            );
            self.out.push("\\end{figure}\n");
        }
    }

    /// `\caption[short]{long}\label{id}`, the labels after the caption so
    /// they capture the figure number; bare `\label`s without a caption.
    fn caption_line(
        &mut self,
        command: &str,
        caption: Option<&str>,
        short: Option<&str>,
        labels: &[String],
        node: Option<&Caption>,
    ) {
        if let Some(node) = node {
            self.out.begin(node.meta.id);
        }
        match caption {
            Some(caption) => {
                self.out.push(command);
                if let Some(short) = short {
                    self.out.push(&format!("[{short}]"));
                }
                self.out.push(&format!("{{{caption}}}"));
                for label in labels {
                    self.out
                        .push(&format!("\\label{{{}}}", escape::label(label)));
                }
                self.out.push("\n");
            }
            None => {
                for label in labels {
                    self.out
                        .push(&format!("\\label{{{}}}\n", escape::label(label)));
                }
            }
        }
        if let Some(node) = node {
            self.out.end(node.meta.id);
        }
    }

    /// `::: figure`: one image is a figure; several are sub-figures
    /// (`subcaption`); a table inside takes the figure's caption
    /// (`writer.py:1024-1039`).
    pub(crate) fn figure(&mut self, f: &Figure, caption: Option<&Caption>) {
        let (content, inner) = match f.content.split_last() {
            Some((Block::Caption(c), rest)) if caption.is_none() => (rest, Some(c)),
            _ => (f.content.as_slice(), None),
        };
        let caption = caption.or(inner);
        let label = f
            .attrs
            .id
            .clone()
            .or_else(|| caption.and_then(|c| c.attrs.id.clone()));
        // The images the resolution turned into sub-figures, in its own
        // order (spec §Image, Figure).
        let images = tmark_registry::figure_images(f);
        if let Some(Block::Table(t)) = content.first() {
            if content.len() == 1 {
                let table = if t.attrs.id.is_none() && label.is_some() {
                    let mut t = t.clone();
                    t.attrs.id = label.clone();
                    t
                } else {
                    t.clone()
                };
                self.table(&table, None, caption);
                return;
            }
        }
        match images.as_slice() {
            [image] => self.figure_image(image, caption, None, label.as_deref()),
            [_, _, ..] => self.subfigures(f, &images, caption, label.as_deref()),
            [] => {
                // Arbitrary content: a float around the blocks.
                self.req.package("float");
                self.out.push("\\begin{figure}[H]\n\\centering\n");
                self.contained(|w| w.blocks(content));
                self.out.ensure_newline();
                let text = caption.map(|c| self.render_inlines(&c.content));
                self.caption_line(
                    "\\caption",
                    text.as_deref(),
                    None,
                    &label.iter().cloned().collect::<Vec<_>>(),
                    caption,
                );
                self.out.push("\\end{figure}\n");
            }
        }
    }

    /// Sub-figures side by side: `cols` per row (default all on one row),
    /// each captioned by its alt and anchored by its own id.
    fn subfigures(
        &mut self,
        f: &Figure,
        images: &[&Image],
        caption: Option<&Caption>,
        label: Option<&str>,
    ) {
        self.req.package("graphicx");
        self.req.package("float");
        self.req.package("subcaption");
        let cols = f
            .attrs
            .get("cols")
            .and_then(|c| c.parse::<usize>().ok())
            .filter(|c| *c > 0)
            .unwrap_or(images.len())
            .min(images.len());
        let share = percent_linewidth((1.0 / cols as f64 - 0.02).max(0.1));
        self.out.push("\\begin{figure}[H]\n\\centering\n");
        for (i, image) in images.iter().enumerate() {
            if i > 0 {
                if i % cols == 0 {
                    self.out.push("\\par\\medskip\n");
                } else {
                    self.out.push("\\hfill\n");
                }
            }
            self.req.assets.push(crate::AssetRef {
                src: image.src.clone(),
                node: image.meta.id,
                attrs: image.attrs.kv.clone(),
            });
            self.out.begin(image.meta.id);
            self.out
                .push(&format!("\\begin{{subfigure}}{{{share}}}\n\\centering\n"));
            self.out.push(&format!(
                "\\includegraphics[width={}]{{{}}}\n",
                width(image.attrs.get("width"), "\\linewidth"),
                escape::escape(strip_theme_variant(&image.src))
            ));
            // Always a `\caption`, empty alt or not: `subcaption` prints
            // the `(a)` marker from it and `\ref` then reads `2a` — the
            // number the resolution gives the sub-figure.
            let alt = self.render_inlines(&image.alt);
            self.out.push(&format!("\\caption{{{alt}}}"));
            if let Some(id) = image.attrs.id() {
                self.out.push(&format!("\\label{{{}}}", escape::label(id)));
            }
            self.out.push("\n");
            self.out.push("\\end{subfigure}\n");
            self.out.end(image.meta.id);
        }
        let text = caption.map(|c| self.render_inlines(&c.content));
        let labels: Vec<String> = label.map(str::to_string).into_iter().collect();
        self.caption_line("\\caption", text.as_deref(), None, &labels, caption);
        self.out.push("\\end{figure}\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(width(Some("50%"), "\\linewidth"), "0.5\\linewidth");
        assert_eq!(width(Some("100%"), "\\linewidth"), "\\linewidth");
        assert_eq!(width(Some("33.3%"), "\\linewidth"), "0.333\\linewidth");
        assert_eq!(width(Some("4cm"), "\\linewidth"), "4cm");
        assert_eq!(width(None, "\\linewidth"), "\\linewidth");
        assert_eq!(strip_theme_variant("a.png#only-dark"), "a.png");
        assert_eq!(strip_theme_variant("a.png#x"), "a.png#x");
    }
}
