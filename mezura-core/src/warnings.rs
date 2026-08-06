// What the run wants the caller to know without it being an error: an answer was produced, and this
// says what to be careful about in it.

// The 'code' of a warning, which is the half a caller can key on
pub const EXTENSION_TIEBREAK        : &str = "extension-tiebreak";
pub const UNKNOWN_FORCED_LANGUAGE   : &str = "unknown-forced-language";
pub const UNKNOWN_LANGUAGE          : &str = "unknown-language";
pub const UNKNOWN_EXCLUDED_LANGUAGE : &str = "unknown-excluded-language";
pub const DUPLICATE_LANGUAGE        : &str = "duplicate-language";
pub const UNUSABLE_LANGUAGE         : &str = "unusable-language";
pub const LANGUAGE_FILE_UNREADABLE  : &str = "language-file-unreadable";
pub const PRIORITY_LINE_SKIPPED     : &str = "priority-line-skipped";
pub const CONFIG_VALUE_IGNORED      : &str = "config-value-ignored";
pub const CONFIG_SECTION_UNKNOWN    : &str = "config-section-unknown";
pub const CONFIG_STYLE_INVALID      : &str = "config-style-invalid";
pub const THEME_UNAVAILABLE         : &str = "theme-unavailable";

// The four fields are for different readers. 'code' never changes, so a script can key on it, while
// 'message' is free to be reworded. 'subject' is the one thing the warning is about, so nobody has to
// dig it back out of the message with a regular expression.
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

// Whether the numbers can be trusted, which is the question worth answering without knowing every
// code: one written against this keeps working when a later version adds a code it never heard of.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum Affects {
    // The counts themselves may be wrong or incomplete.
    Counts,
    // The counts are sound, but something that was asked for was not applied.
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

#[cfg(test)]
mod tests {
    use super::*;

    // A code that never changes beside wording that is free to change is the whole reason for the
    // type, so both halves are asserted.
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
