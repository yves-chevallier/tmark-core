//! Byte-level predicates shared by the TMark constructs.
//!
//! They are public so that `tmark-syntax` applies the same rules when it
//! lowers the tree (one definition of "what an attribute list looks like").

/// Whether the character before `index` is a word character for the X4
/// guard (`@` never fires inside a word): alphanumeric in the Unicode sense,
/// or `_`. `§`, `(`, `«` and other punctuation are not.
pub fn word_char_before(bytes: &[u8], index: usize) -> bool {
    crate::util::char::before_index(bytes, index).is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Whether a byte can start an identifier (`[A-Za-z]`).
pub fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

/// Whether a byte can continue an identifier (`[A-Za-z0-9_-]`).
pub fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Whether `bytes[index]` is a `{` that starts an attribute list: after
/// an optional `:` (the deprecated Python-Markdown `attr_list` colon, spec
/// §Attributes) and optional blanks comes `#x`, `.x`, or `key=`.
pub fn looks_like_attributes(bytes: &[u8], index: usize) -> bool {
    if bytes.get(index) != Some(&b'{') {
        return false;
    }
    let mut i = index + 1;
    if bytes.get(i) == Some(&b':') {
        i += 1;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'#' | b'.') => {
            matches!(bytes.get(i + 1), Some(b) if !b.is_ascii_whitespace() && *b != b'}')
        }
        Some(b) if is_ident_byte(*b) => {
            let mut j = i;
            while matches!(bytes.get(j), Some(b) if is_ident_byte(*b)) {
                j += 1;
            }
            bytes.get(j) == Some(&b'=')
        }
        _ => false,
    }
}

/// Whether the line at `index` (its first non-blank byte) is a foreign
/// directive head (spec §Foreign directive): three or more `:`, optional
/// blanks, a dotted name `ident(.ident)+`, optional blanks, then the end of
/// the line. Shared by the tokenizer (which takes the line plus its
/// indented body) and `tmark-syntax`.
pub fn is_foreign_directive(bytes: &[u8], index: usize) -> bool {
    let mut i = index;
    let mut colons = 0;
    while bytes.get(i) == Some(&b':') {
        i += 1;
        colons += 1;
    }
    if colons < 3 {
        return false;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let mut dots = 0;
    loop {
        if !bytes.get(i).is_some_and(|b| is_ident_start(*b)) {
            return false;
        }
        while bytes.get(i).is_some_and(|b| is_ident_byte(*b)) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            dots += 1;
            i += 1;
            continue;
        }
        break;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    dots > 0 && matches!(bytes.get(i), None | Some(b'\n'))
}

/// Index of the end of the current line: the `\n`, or the end of input.
pub fn line_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

/// Whether the display-math fence at `index` closes its block at the end of
/// a content line (spec §Math (display); Pandoc and `python-markdown-math`
/// spell a display as `$$…$$`, the closing fence hugging the last line:
/// `\end{matrix} \right.$$`).
///
/// True when `bytes[index]` starts a run of at least `size` `$`, followed
/// by, up to the end of the line, only blanks and at most one attribute
/// list (`$$ {#eq:x}`), *and* the line already holds content: a `$$` alone
/// on its line is the ordinary closing fence, which the fence states
/// handle — including their indent rules (four spaces is content, not a
/// fence).
pub fn math_closes_line(bytes: &[u8], index: usize, size: usize, attributes: bool) -> bool {
    // Content before it on this line?
    let mut before = index;
    loop {
        if before == 0 {
            return false;
        }
        before -= 1;
        match bytes[before] {
            b'\n' => return false,
            b' ' | b'\t' => {}
            _ => break,
        }
    }
    let mut i = index;
    let mut dollars = 0;
    while bytes.get(i) == Some(&b'$') {
        i += 1;
        dollars += 1;
    }
    if dollars < size {
        return false;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let end = line_end(bytes, i);
    if attributes && looks_like_attributes(bytes, i) {
        if let Some(offset) = bytes[i..end].iter().position(|b| *b == b'}') {
            i += offset + 1;
            while matches!(bytes.get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
        }
    }
    i == end
}
