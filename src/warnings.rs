use std::sync::{Mutex, OnceLock};

// What a warning puts in doubt, which is the question a machine consumer actually has. A CI gate
// asks "can I trust these numbers", not "which of the eleven codes are the serious ones", and this
// is what lets one written today keep working when a later version adds a code it never heard of.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum Affects {
    // The counts themselves may be wrong or incomplete
    Counts,
    // The counts are sound, but something that was asked for was not applied
    Settings
}

impl Affects {
    pub fn name(self) -> &'static str {
        match self {
            Self::Counts => "counts",
            Self::Settings => "settings"
        }
    }
}

// 'code' is the stable half and 'message' is the readable one, which is the whole reason for having
// both: the wording is free to improve without breaking anything that reads the document, and a
// reader of the terminal never has to look a number up. 'subject' is the one thing the warning is
// about, so that nobody has to pull it back out of the message with a regular expression.
#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Warning {
    pub code: &'static str,
    pub affects: Affects,
    pub subject: String,
    pub message: String
}

impl Warning {
    pub fn new(code: &'static str, affects: Affects, subject: &str, message: String) -> Self {
        Warning { code, affects, subject: subject.to_owned(), message }
    }
}

pub const EXTENSION_TIEBREAK      : &str = "extension-tiebreak";
pub const UNKNOWN_FORCED_LANGUAGE : &str = "unknown-forced-language";
pub const LANGUAGE_FILE_UNREADABLE: &str = "language-file-unreadable";
pub const PRIORITY_LINE_SKIPPED   : &str = "priority-line-skipped";
pub const CONFIG_VALUE_IGNORED    : &str = "config-value-ignored";
pub const CONFIG_SECTION_UNKNOWN  : &str = "config-section-unknown";
pub const CONFIG_STYLE_INVALID    : &str = "config-style-invalid";
pub const THEME_UNAVAILABLE       : &str = "theme-unavailable";

// A run emits these from four modules and from phases that do not share a value between them: some
// before a configuration exists at all, some from inside the counting. Threading a sink through all
// of it would mean a parameter on every signature between here and there for the sake of one list,
// so it is held the way the active theme and the separators already are.
static EMITTED : OnceLock<Mutex<Vec<Warning>>> = OnceLock::new();

fn emitted() -> &'static Mutex<Vec<Warning>> {
    EMITTED.get_or_init(|| Mutex::new(Vec::new()))
}

// Printed and kept in one call, so that the terminal and the document can never end up saying two
// different things about the same warning. What is printed is exactly what was printed before any
// of this existed, down to the leading blank line.
pub fn emit(warning: Warning) {
    eprintln!("\n{}", crate::theme::active().warning.paint(&warning.message));
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

    // The collector is shared by the whole process, so this asks whether its own warning arrived
    // rather than counting what is in there: the other tests of this binary run beside it and emit
    // their own.
    #[test]
    fn a_warning_is_kept_with_both_of_its_halves() {
        keep(Warning::new(EXTENSION_TIEBREAK, Affects::Counts, "a-subject-no-other-test-uses",
                "the readable half".to_owned()));

        let mine = collected().into_iter().find(|x| x.subject == "a-subject-no-other-test-uses").unwrap();
        assert_eq!(EXTENSION_TIEBREAK, mine.code);
        assert_eq!("counts", mine.affects.name());
        assert_eq!("the readable half", mine.message);
        assert_eq!("settings", Affects::Settings.name());
    }
}
