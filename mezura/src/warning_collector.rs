use std::sync::Mutex;

use mezura_core::warnings::{Affects, Code, Warning};

// A global because warnings arrive from phases that share no value between them, and some of them
// run before a configuration exists at all.
static EMITTED : Mutex<Vec<ProgramWarning>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramCode {
    LanguageFileUnreadable,
    ConflictLineSkipped,
    ConfigValueIgnored,
    ConfigSectionUnknown,
    ConfigSectionRepeated,
    CommandIgnored,
    ConfigStyleInvalid,
    ThemeUnavailable,
    PresetFileShadowed
}

impl ProgramCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LanguageFileUnreadable => "language-file-unreadable",
            Self::ConflictLineSkipped => "conflict-line-skipped",
            Self::ConfigValueIgnored => "config-value-ignored",
            Self::ConfigSectionUnknown => "config-section-unknown",
            Self::ConfigSectionRepeated => "config-section-repeated",
            Self::CommandIgnored => "command-ignored",
            Self::ConfigStyleInvalid => "config-style-invalid",
            Self::ThemeUnavailable => "theme-unavailable",
            Self::PresetFileShadowed => "preset-file-shadowed"
        }
    }

    pub fn affects(self) -> Affects {
        match self {
            Self::LanguageFileUnreadable => Affects::Counts,
            Self::ConflictLineSkipped | Self::ConfigValueIgnored | Self::ConfigSectionUnknown
            | Self::ConfigSectionRepeated | Self::CommandIgnored | Self::ConfigStyleInvalid
            | Self::ThemeUnavailable | Self::PresetFileShadowed => Affects::Settings
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyCode {
    Library(Code),
    Program(ProgramCode)
}

impl AnyCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library(code) => code.as_str(),
            Self::Program(code) => code.as_str()
        }
    }

    pub fn affects(self) -> Affects {
        match self {
            Self::Library(code) => code.affects(),
            Self::Program(code) => code.affects()
        }
    }
}

impl From<Code> for AnyCode {
    fn from(code: Code) -> Self {
        AnyCode::Library(code)
    }
}

impl From<ProgramCode> for AnyCode {
    fn from(code: ProgramCode) -> Self {
        AnyCode::Program(code)
    }
}

#[derive(Debug, Clone)]
pub struct ProgramWarning {
    pub code: AnyCode,
    pub subject: String,
    pub message: String
}

impl ProgramWarning {
    pub fn new(code: impl Into<AnyCode>, subject: &str, message: String) -> Self {
        ProgramWarning { code: code.into(), subject: subject.to_owned(), message }
    }

    pub fn affects(&self) -> Affects {
        self.code.affects()
    }
}

impl From<Warning> for ProgramWarning {
    fn from(warning: Warning) -> Self {
        ProgramWarning { code: AnyCode::Library(warning.code), subject: warning.subject, message: warning.message }
    }
}

// Printed and kept in one call, so the terminal and the document cannot end up saying two different
// things about the same warning.
pub fn emit(warning: ProgramWarning) {
    let warning = add_advice_to(warning);
    eprintln!("\n{}", super::theme::get_active().warning
            .paint(&super::message_printer::wrap_message(&warning.message)));
    EMITTED.lock().unwrap().push(warning);
}

// For the places that print in a shape of their own, and would say it twice if this printed too
pub fn keep(warning: ProgramWarning) {
    EMITTED.lock().unwrap().push(add_advice_to(warning));
}

pub fn get_collected_warnings() -> Vec<ProgramWarning> {
    EMITTED.lock().unwrap().clone()
}

// An unknown name was already put on the screen by 'report_unknown_languages', with a suggested
// spelling under it, so that one is only kept for the document
pub fn report_language_resolution_warnings(reported: Vec<Warning>) {
    for warning in reported {
        if warning.code == Code::UnknownLanguage {
            keep(warning.into());
        } else {
            emit(warning.into());
        }
    }
}

// What a mezura user can do about it, added to the sentence the library wrote. The library cannot
// say it: a program counting through mezura-core has no data directory and no command line, and
// would be told to do something it cannot.
fn add_advice_to(mut warning: ProgramWarning) -> ProgramWarning {
    let AnyCode::Library(code) = warning.code else { return warning };
    // Deliberately not exhaustive: a new code arriving without a line of advice is a missing nicety
    // and not a wrong answer.
    let advice = match code {
        // The conflicts file has no block for interpreters. A contested shebang written into it is
        // skipped without a word, so sending its owner there would be advice that fails silently.
        Code::LanguageTiebreak => Some(format!("A contested extension or filename is settled for good in '{}', \
                and '--force-language {}=<language>' decides it for this run.",
                mezura_core::LANGUAGE_CONFLICTS_FILE_NAME, warning.subject)),
        Code::DuplicateLanguage => Some("Delete the copies you do not want from the 'languages' directory of your data directory.".to_owned()),
        Code::LanguageLostEveryClaim => Some(format!("A line in '{}' hands an extension back, to whichever \
                name it puts first.", mezura_core::LANGUAGE_CONFLICTS_FILE_NAME)),
        Code::UnknownForcedLanguage => Some("Run with '--show-languages' for the ones available to '--force-language'.".to_owned()),
        Code::UnknownLanguage => Some("Run with '--show-languages' for the ones available to '--languages'.".to_owned()),
        Code::UnknownExcludedLanguage =>
                Some("Run with '--show-languages' for the ones available to '--exclude-languages'.".to_owned()),
        Code::UnknownSectionLanguage =>
                Some("Correct the 'Nested language default' line of that language file in your data dir.".to_owned()),
        Code::UnknownModuleScope => Some(format!("Name that module in '--{}', as '{}=<path>', or drop it from \
                in front of the rule to have the rule hold everywhere.",
                super::config_manager::TARGETS, warning.subject)),
        _ => None
    };

    if let Some(advice) = advice {
        warning.message = format!("{}\n{advice}", warning.message);
    }
    warning
}

#[cfg(test)]
mod tests {
    use super::*;

    // The collector is shared by the whole process and the other tests run beside this one, so it
    // looks for its own warning rather than counting what is in there.
    #[test]
    fn a_kept_warning_reaches_the_collector() {
        keep(ProgramWarning::new(Code::LanguageTiebreak, "a-subject-no-other-test-uses",
                "the readable half".to_owned()));

        let mine = get_collected_warnings().into_iter().find(|x| x.subject == "a-subject-no-other-test-uses").unwrap();
        assert_eq!(AnyCode::Library(Code::LanguageTiebreak), mine.code);
        assert!(mine.message.starts_with("the readable half"), "{}", mine.message);
    }

    #[test]
    fn the_advice_this_program_can_give_is_added_to_the_librarys_sentence() {
        let advised = add_advice_to(ProgramWarning::new(Code::LanguageTiebreak, "m",
                "The extension 'm' is claimed by MATLAB and Objective-C.".to_owned()));

        assert!(advised.message.starts_with("The extension 'm' is claimed by MATLAB and Objective-C.\n"),
                "the library's own sentence was not kept whole:\n{}", advised.message);
        assert!(advised.message.contains("--force-language m=<language>"), "{}", advised.message);
        assert!(advised.message.contains("language_conflicts.txt"), "{}", advised.message);

        for (code, command) in [(Code::UnknownForcedLanguage, "--force-language"), (Code::UnknownLanguage, "--languages"),
                (Code::UnknownExcludedLanguage, "--exclude-languages")] {
            let advised = add_advice_to(ProgramWarning::new(code, "zz", "not found.".to_owned()));
            assert!(advised.message.contains(command), "{} did not name '{command}':\n{}", code.as_str(), advised.message);
        }

        let untouched = add_advice_to(ProgramWarning::new(ProgramCode::ConflictLineSkipped, "a line",
                "that line was skipped.".to_owned()));
        assert_eq!("that line was skipped.", untouched.message);
    }
}
