// Warnings are values by the time they reach here. Collecting them for the document and putting them
// on the screen are both about showing, so both live on this side.
use std::sync::{Mutex, OnceLock};

use mezura::warnings::Warning;

// Held the way the active theme and the separators are, because these arrive from phases that share
// no value between them: some before a configuration exists at all. Threading a sink through every
// signature between here and there, for the sake of one list, buys nothing.
static EMITTED : OnceLock<Mutex<Vec<Warning>>> = OnceLock::new();

fn emitted() -> &'static Mutex<Vec<Warning>> {
    EMITTED.get_or_init(|| Mutex::new(Vec::new()))
}

// Printed and kept in one call, so that the terminal and the document can never end up saying two
// different things about the same warning. What is printed is exactly what was printed before any
// of this existed, down to the leading blank line.
pub fn emit(warning: Warning) {
    eprintln!("\n{}", super::theme::active().warning.paint(&warning.message));
    keep(warning);
}

// For the places that print in a shape of their own, and would say it twice if this printed too
pub fn keep(warning: Warning) {
    emitted().lock().unwrap().push(warning);
}

pub fn collected() -> Vec<Warning> {
    emitted().lock().unwrap().clone()
}


#[cfg(test)]
mod tests {
    use super::*;
    use mezura::warnings::{Affects, EXTENSION_TIEBREAK};

    // The collector is shared by the whole process, so this asks whether its own warning arrived
    // rather than counting what is in there: the other tests of this binary run beside it and keep
    // their own.
    #[test]
    fn a_kept_warning_reaches_the_collector() {
        keep(Warning::new(EXTENSION_TIEBREAK, Affects::Counts, "a-subject-no-other-test-uses",
                "the readable half".to_owned()));

        let mine = collected().into_iter().find(|x| x.subject == "a-subject-no-other-test-uses").unwrap();
        assert_eq!(EXTENSION_TIEBREAK, mine.code);
        assert_eq!("the readable half", mine.message);
    }
}
