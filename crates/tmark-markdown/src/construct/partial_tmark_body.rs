//! Indented body shared by the TMark admonition and definition constructs.
//!
//! The body is the run of lines that follow the head line and are either
//! blank or indented by [`TAB_SIZE`][] (spaces or a tab), the indent
//! stripped. It mirrors [code (indented)][crate::construct::code_indented];
//! the chunk token comes from `tokenize_state.token_1`.
//!
//! ```markdown
//! > | !!! note "Title"
//! > |     first body line
//! > |
//! > |     second body paragraph
//! ```

use crate::construct::partial_space_or_tab::{space_or_tab, space_or_tab_min_max};
use crate::state::{Name as StateName, State};
use crate::tokenizer::Tokenizer;
use crate::util::constant::TAB_SIZE;

/// At a line break, or at the start of a body line after its indent.
pub fn at_break(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        None => State::Ok,
        Some(b'\n') => {
            tokenizer.attempt(State::Next(StateName::TmarkBodyAtBreak), State::Ok);
            State::Retry(StateName::TmarkBodyFurtherStart)
        }
        _ => {
            tokenizer.enter(tokenizer.tokenize_state.token_1.clone());
            State::Retry(StateName::TmarkBodyInside)
        }
    }
}

/// Inside a body line.
pub fn inside(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        None | Some(b'\n') => {
            tokenizer.exit(tokenizer.tokenize_state.token_1.clone());
            State::Retry(StateName::TmarkBodyAtBreak)
        }
        _ => {
            tokenizer.consume();
            State::Next(StateName::TmarkBodyInside)
        }
    }
}

/// At a line ending, looking for a further indented line (blank lines may
/// come between).
pub fn further_start(tokenizer: &mut Tokenizer) -> State {
    if tokenizer.current == Some(b'\n') {
        // At the line ending that closes the previous line: `lazy` and
        // `pierce` still describe *that* line, not the body line we are
        // about to read. A head line that closed a container is lazy
        // (`- a`, a blank line, `!!! note`): its body follows all the same.
        // The flags are checked when this state is re-entered, at the start
        // of the next line, where they describe it.
        tokenizer.enter(crate::event::Name::LineEnding);
        tokenizer.consume();
        tokenizer.exit(crate::event::Name::LineEnding);
        State::Next(StateName::TmarkBodyFurtherStart)
    } else if tokenizer.lazy || tokenizer.pierce {
        State::Nok
    } else {
        tokenizer.attempt(State::Ok, State::Next(StateName::TmarkBodyFurtherBegin));
        State::Retry(space_or_tab_min_max(tokenizer, TAB_SIZE, TAB_SIZE))
    }
}

/// At a line that is not indented enough: only fine if blank.
pub fn further_begin(tokenizer: &mut Tokenizer) -> State {
    if matches!(tokenizer.current, Some(b'\t' | b' ')) {
        tokenizer.attempt(State::Next(StateName::TmarkBodyFurtherAfter), State::Nok);
        State::Retry(space_or_tab(tokenizer))
    } else {
        State::Nok
    }
}

/// After the whitespace of a blank line.
pub fn further_after(tokenizer: &mut Tokenizer) -> State {
    match tokenizer.current {
        Some(b'\n') => State::Retry(StateName::TmarkBodyFurtherStart),
        _ => State::Nok,
    }
}
