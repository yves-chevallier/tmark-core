//! The front-matter epigraph (spec §BlockQuote): `epigraph: {quote,
//! source}` is the same node a `> {.epigraph}` quote is, placed under the
//! document's opening heading.
//!
//! The writers that walk the block list take [`blocks`], which hands them
//! the document's blocks with that node spliced in, so each of them keeps
//! exactly one epigraph emitter. The web lowering splices source text
//! rather than nodes and reads [`front_matter`] instead.

use std::borrow::Cow;

use tmark_ir::{Attrs, Block, BlockQuote, Document, Epigraph, Inline, Meta, Para, Str};

/// The document's front-matter epigraph, when it has one with a quote.
pub fn front_matter(doc: &Document) -> Option<&Epigraph> {
    doc.front_matter
        .keys
        .epigraph
        .as_ref()
        .filter(|e| !e.quote.trim().is_empty())
}

/// Where the epigraph goes: after the document's opening heading, else at
/// the top. "Opening" is the first top-level block, so a heading further
/// down the page is a section of its own and takes none.
pub fn index(doc: &Document) -> usize {
    match doc.blocks.first() {
        Some(Block::Header(_)) => 1,
        _ => 0,
    }
}

/// The epigraph as the block quote every writer already renders.
///
/// It carries the front matter's own node id and span: that island is
/// where the text comes from, so a source map points a reader at it.
pub fn block(doc: &Document) -> Option<Block> {
    let epigraph = front_matter(doc)?;
    let meta = Meta::new(doc.front_matter.meta.id, doc.front_matter.meta.span);
    let mut attrs = Attrs::new();
    attrs.classes.push("epigraph".to_string());
    if let Some(source) = epigraph.source.as_deref().filter(|s| !s.trim().is_empty()) {
        attrs.kv.push(("source".to_string(), source.to_string()));
    }
    Some(Block::BlockQuote(BlockQuote {
        meta,
        content: vec![Block::Para(Para {
            meta,
            content: vec![Inline::Str(Str {
                meta,
                text: epigraph.quote.clone(),
            })],
            lead: None,
        })],
        attrs,
    }))
}

/// The document's blocks, the epigraph spliced in at [`index`]; the
/// blocks themselves when there is none.
pub fn blocks(doc: &Document) -> Cow<'_, [Block]> {
    match block(doc) {
        None => Cow::Borrowed(&doc.blocks),
        Some(quote) => {
            let at = index(doc);
            let mut blocks = doc.blocks.clone();
            blocks.insert(at, quote);
            Cow::Owned(blocks)
        }
    }
}
