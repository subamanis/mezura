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
#[non_exhaustive]
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
pub const UNKNOWN_LANGUAGE        : &str = "unknown-language";
pub const UNKNOWN_EXCLUDED_LANGUAGE: &str = "unknown-excluded-language";
pub const LANGUAGE_FILE_UNREADABLE: &str = "language-file-unreadable";
pub const PRIORITY_LINE_SKIPPED   : &str = "priority-line-skipped";
pub const CONFIG_VALUE_IGNORED    : &str = "config-value-ignored";
pub const CONFIG_SECTION_UNKNOWN  : &str = "config-section-unknown";
pub const CONFIG_STYLE_INVALID    : &str = "config-style-invalid";
pub const THEME_UNAVAILABLE       : &str = "theme-unavailable";


#[cfg(test)]
mod tests {
    use super::*;

    // The two halves are the whole reason the type exists: a code that never changes and wording
    // that is free to improve.
    #[test]
    fn a_warning_carries_a_stable_code_and_a_readable_message() {
        let warning = Warning::new(EXTENSION_TIEBREAK, Affects::Counts, "m", "the readable half".to_owned());

        assert_eq!(EXTENSION_TIEBREAK, warning.code);
        assert_eq!("counts", warning.affects.name());
        assert_eq!("m", warning.subject);
        assert_eq!("the readable half", warning.message);
        assert_eq!("settings", Affects::Settings.name());
    }
}
