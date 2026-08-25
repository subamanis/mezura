// What the run wants the caller to know without it being an error: an answer was produced, and this
// says what to be careful about in it.

#[derive(Debug,PartialEq,Eq,Clone,Copy)]
#[non_exhaustive]
pub enum Code {
    LanguageTiebreak,
    UnknownForcedLanguage,
    UnknownLanguage,
    UnknownExcludedLanguage,
    // A rule was written for a module that no target of this run declares, so it settled nothing
    UnknownModuleScope,
    // A section of another language falls back to the file's own language, so its comments are read
    // with the wrong symbols
    UnknownSectionLanguage,
    DuplicateLanguage,
    // Every file carrying its extensions is left to be counted by nobody
    LanguageWithoutName,
    // Nothing is lost: it claims neither an extension nor a filename, so no file could ever match it
    LanguageClaimsNothing,
    // A block comment whose opening and closing symbols are the same text, which the scan cannot
    // tell apart, so the pair never fires and its comments are counted as code
    CommentPairNeverCloses,
    LanguageFileUnreadable,
    ConflictLineSkipped,
    ConfigValueIgnored,
    ConfigSectionUnknown,
    CommandIgnored,
    ConfigStyleInvalid,
    ThemeUnavailable
}

impl Code {
    pub fn name(self) -> &'static str {
        match self {
            Self::LanguageTiebreak => "language-tiebreak",
            Self::UnknownForcedLanguage => "unknown-forced-language",
            Self::UnknownLanguage => "unknown-language",
            Self::UnknownExcludedLanguage => "unknown-excluded-language",
            Self::UnknownModuleScope => "unknown-module-scope",
            Self::UnknownSectionLanguage => "unknown-section-language",
            Self::DuplicateLanguage => "duplicate-language",
            Self::LanguageWithoutName => "language-without-name",
            Self::LanguageClaimsNothing => "language-claims-nothing",
            Self::CommentPairNeverCloses => "comment-pair-never-closes",
            Self::LanguageFileUnreadable => "language-file-unreadable",
            Self::ConflictLineSkipped => "conflict-line-skipped",
            Self::ConfigValueIgnored => "config-value-ignored",
            Self::ConfigSectionUnknown => "config-section-unknown",
            Self::CommandIgnored => "command-ignored",
            Self::ConfigStyleInvalid => "config-style-invalid",
            Self::ThemeUnavailable => "theme-unavailable"
        }
    }

    // Exhaustive on purpose: a code added without deciding what it does to the answer does not
    // compile.
    pub fn affects(self) -> Affects {
        match self {
            Self::LanguageTiebreak | Self::DuplicateLanguage | Self::LanguageWithoutName
            | Self::LanguageFileUnreadable | Self::UnknownSectionLanguage
            | Self::CommentPairNeverCloses => Affects::Counts,
            Self::UnknownForcedLanguage | Self::UnknownLanguage | Self::UnknownExcludedLanguage
            | Self::UnknownModuleScope
            | Self::LanguageClaimsNothing | Self::ConflictLineSkipped | Self::ConfigValueIgnored
            | Self::ConfigSectionUnknown | Self::CommandIgnored | Self::ConfigStyleInvalid
            | Self::ThemeUnavailable => Affects::Settings
        }
    }
}

// The three fields are for different readers. 'code' never changes, so a script can key on it, while
// 'message' is free to be reworded. 'subject' is the one thing the warning is about, so nobody has to
// dig it back out of the message with a regular expression.
#[derive(Debug,PartialEq,Eq,Clone)]
#[non_exhaustive]
pub struct Warning {
    pub code: Code,
    pub subject: String,
    pub message: String
}

impl Warning {
    pub fn new(code: Code, subject: &str, message: String) -> Self {
        Warning { code, subject: subject.to_owned(), message }
    }

    pub fn affects(&self) -> Affects {
        self.code.affects()
    }
}

// Whether the numbers can be trusted, which is the question worth answering without knowing every
// code: one written against this keeps working when a later version adds a code it never heard of.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum Affects {
    // The numbers are wrong for the settings that were applied. Rewriting the command does not fix
    // it, and nothing should be decided on figures raised with this.
    Counts,
    // The numbers are sound for what was applied, but what was applied is not what was asked for.
    // Rewriting the command does fix it.
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

    #[test]
    fn a_warning_carries_a_stable_code_and_a_readable_message() {
        let warning = Warning::new(Code::LanguageTiebreak, "m", "the readable half".to_owned());

        assert_eq!("language-tiebreak", warning.code.name());
        assert_eq!("counts", warning.affects().name());
        assert_eq!("settings", Affects::Settings.name());
    }

    #[test]
    fn a_language_that_cannot_be_named_puts_the_counts_in_doubt_and_one_that_claims_nothing_does_not() {
        assert_eq!(Affects::Counts, Code::LanguageWithoutName.affects());
        assert_eq!(Affects::Settings, Code::LanguageClaimsNothing.affects());
    }
}
