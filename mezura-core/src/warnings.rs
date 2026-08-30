//! What a run wants the caller to know without it being an error: an answer was produced, and this
//! says what to be careful about in it.

/// What a warning is about. Stable, so a script can key on it: the wording of a message is free to
/// change, this is not.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
#[non_exhaustive]
pub enum Code {
    /// Two languages claim one extension and no rule settles it, so it went to the alphabetically
    /// first of the two.
    LanguageTiebreak,
    /// An extension was handed to a language name nothing answers to.
    UnknownForcedLanguage,
    /// A name given to the languages of interest matches no language.
    UnknownLanguage,
    /// The same, for a name given to the excluded languages.
    UnknownExcludedLanguage,
    /// A rule was written for a module that no target of this run declares, so it settled nothing.
    UnknownModuleScope,
    /// A section of another language falls back to the file's own language, so its comments are
    /// read with the wrong symbols.
    UnknownSectionLanguage,
    /// Two language files carry the same name and only one of them is in play.
    DuplicateLanguage,
    /// A language definition carries no name, so every file claiming its extensions is left to be
    /// counted by nobody.
    LanguageWithoutName,
    /// A language claims no extension, no file name and no `#!` line, so no file could ever be
    /// counted as it. Nothing is lost.
    LanguageClaimsNothing,
    /// Every extension and name a language claims went to another one, so no file can reach it.
    LanguageLostEveryClaim,
    /// A block comment whose opening and closing symbols are the same text, which the scan cannot
    /// tell apart, so the pair never fires and its comments are counted as code.
    CommentPairNeverCloses,
    /// A language file could not be read or does not parse, so that whole language is missing.
    LanguageFileUnreadable,
    /// A line of the file that settles contested extensions does not parse and was skipped.
    ConflictLineSkipped,
    /// A configuration file holds a value that could not be used.
    ConfigValueIgnored,
    /// A configuration file holds a section this version does not know.
    ConfigSectionUnknown,
    /// A configuration file declares the same section more than once.
    ConfigSectionRepeated,
    /// A command was given that this run has no use for.
    CommandIgnored,
    /// A style line does not parse and was skipped, the rest of its file applying.
    ConfigStyleInvalid,
    /// The theme that was asked for is not installed, so the default was used.
    ThemeUnavailable
}

impl Code {
    /// The stable spelling, `language-tiebreak`.
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
            Self::LanguageLostEveryClaim => "language-lost-every-claim",
            Self::CommentPairNeverCloses => "comment-pair-never-closes",
            Self::LanguageFileUnreadable => "language-file-unreadable",
            Self::ConflictLineSkipped => "conflict-line-skipped",
            Self::ConfigValueIgnored => "config-value-ignored",
            Self::ConfigSectionUnknown => "config-section-unknown",
            Self::ConfigSectionRepeated => "config-section-repeated",
            Self::CommandIgnored => "command-ignored",
            Self::ConfigStyleInvalid => "config-style-invalid",
            Self::ThemeUnavailable => "theme-unavailable"
        }
    }

    /// Whether this puts the numbers themselves in doubt.
    // Exhaustive on purpose: a code added without deciding what it does to the answer does not
    // compile.
    pub fn affects(self) -> Affects {
        match self {
            Self::LanguageTiebreak | Self::DuplicateLanguage | Self::LanguageWithoutName
            | Self::LanguageFileUnreadable | Self::UnknownSectionLanguage
            | Self::CommentPairNeverCloses => Affects::Counts,
            Self::UnknownForcedLanguage | Self::UnknownLanguage | Self::UnknownExcludedLanguage
            | Self::UnknownModuleScope
            | Self::LanguageClaimsNothing | Self::LanguageLostEveryClaim
            | Self::ConflictLineSkipped | Self::ConfigValueIgnored
            | Self::ConfigSectionUnknown | Self::ConfigSectionRepeated
            | Self::CommandIgnored | Self::ConfigStyleInvalid
            | Self::ThemeUnavailable => Affects::Settings
        }
    }
}

/// One thing worth knowing about a run that still produced an answer.
///
/// The three fields are for three different readers.
#[derive(Debug,PartialEq,Eq,Clone)]
#[non_exhaustive]
pub struct Warning {
    /// What it is about, in a form a script can key on.
    pub code: Code,
    /// The one thing it concerns, so nobody has to dig it back out of the message with a regular
    /// expression: a language name, an extension, a file name.
    pub subject: String,
    /// The same for a person to read. Free to be reworded between versions.
    pub message: String
}

impl Warning {
    /// A warning of that code, about that subject, worded that way.
    pub fn new(code: Code, subject: &str, message: String) -> Self {
        Warning { code, subject: subject.to_owned(), message }
    }

    /// Whether this puts the numbers themselves in doubt.
    pub fn affects(&self) -> Affects {
        self.code.affects()
    }
}

/// Whether the numbers can be trusted, which is the question worth answering without knowing every
/// code: something written against this keeps working when a later version adds a code it never
/// heard of.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum Affects {
    /// The numbers are wrong for the settings that were applied. Rewriting the command does not fix
    /// it, and nothing should be decided on figures raised with this.
    Counts,
    /// The numbers are sound for what was applied, but what was applied is not what was asked for.
    /// Rewriting the command does fix it.
    Settings
}

impl Affects {
    /// `counts` or `settings`.
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
