//! A `[` that opens a table cell or a text run right after a `\\`
//! (design 07 §Decisions, fixture `table-cell-bracket`). LaTeX's row
//! break and booktabs' rules take an optional `[⟨dimen⟩]` argument and
//! scan for it past the line end, so an unguarded bracket is read as
//! that argument (`Missing number, treated as zero`). Typst escapes `[`
//! in markup and has no such hazard.

use tmark_registry::{resolve, MemoryLoader, ResolveOptions};
use tmark_writers::{write, Backend, Media, WriterOptions};

fn body(md: &str, backend: Backend) -> String {
    let parsed = tmark_syntax::parse(md, tmark_ir::FileId::default());
    let resolved = resolve(
        &parsed.document,
        &MemoryLoader::new(),
        &ResolveOptions::default(),
    );
    let opts = WriterOptions {
        media: Media::Print,
        ..WriterOptions::default()
    };
    write(&parsed.document, &resolved, backend, &opts).text
}

#[test]
fn a_pipe_table_cell_that_opens_with_a_bracket_is_braced() {
    let md = "| a | b |\n| - | - |\n| [x] | y |\n";
    let latex = body(md, Backend::Latex);
    assert!(latex.contains("{[}x] & y \\\\"), "{latex}");
    assert!(!latex.contains("\n[x]"), "{latex}");
    assert!(body(md, Backend::Typst).contains("[\\[x\\]], [y]"));
}

#[test]
fn a_column_name_that_opens_with_a_bracket_is_braced() {
    let md = "| [a] | b |\n| - | - |\n| x | y |\n";
    let latex = body(md, Backend::Latex);
    assert!(latex.contains("\\textbf{{[}a]}"), "{latex}");
}

#[test]
fn a_yaml_table_cell_that_opens_with_a_bracket_is_braced() {
    // The `yaml table` path (`\midrule` and `\addlinespace` take an
    // optional argument too), with a spanning cell so that the
    // `\multicolumn` a row may open with is exercised beside it.
    let md = "```yaml table\ncolumns: [a, b]\nrows:\n  - [{value: wide, cols: 2}]\n  - [\"[x]\", y]\n```\n";
    let latex = body(md, Backend::Latex);
    assert!(latex.contains("\\multicolumn{2}{c}{wide}"), "{latex}");
    assert!(latex.contains("{[}x] & y \\\\"), "{latex}");
}

#[test]
fn a_text_run_after_a_hard_break_that_opens_with_a_bracket_is_guarded() {
    let latex = body("one  \n[note] two\n", Backend::Latex);
    assert!(latex.contains("one\\\\\n{}[note] two"), "{latex}");
    // Nothing is added where no break precedes.
    assert!(body("[note] two\n", Backend::Latex).starts_with("[note] two"));
}

#[test]
fn what_a_node_opens_with_a_bracket_is_guarded_after_a_break_too() {
    // Not only a text run: an unresolved `@key` and an unresolved
    // reference-style link (spec §Ref) both open with `[`, and the `\\`
    // before them scans past the line end all the same.
    let latex = body("one  \n@fig:nope two\n", Backend::Latex);
    assert!(latex.contains("one\\\\\n{}[?fig:nope] two"), "{latex}");
    let latex = body("one  \n[prose][nothing] two\n", Backend::Latex);
    assert!(latex.contains("one\\\\\n{}[prose][nothing] two"), "{latex}");
}
