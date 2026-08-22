use std::sync::{Mutex, OnceLock};

use mezura_core::warnings::Warning;

// A global because warnings arrive from phases that share no value between them, and some of them
// run before a configuration exists at all.
static EMITTED : OnceLock<Mutex<Vec<Warning>>> = OnceLock::new();

// Printed and kept in one call, so the terminal and the document cannot end up saying two different
// things about the same warning.
pub fn emit(warning: Warning) {
    let warning = add_advice_to(warning);
    eprintln!("\n{}", super::theme::get_active().warning
            .paint(&super::message_printer::wrap_message(&warning.message)));
    emitted().lock().unwrap().push(warning);
}

// For the places that print in a shape of their own, and would say it twice if this printed too
pub fn keep(warning: Warning) {
    emitted().lock().unwrap().push(add_advice_to(warning));
}

pub fn get_collected_warnings() -> Vec<Warning> {
    emitted().lock().unwrap().clone()
}

// An unknown name was already put on the screen by 'report_unknown_languages', with a suggested
// spelling under it, so that one is only kept for the document
pub fn report_language_resolution_warnings(reported: Vec<Warning>) {
    for warning in reported {
        if warning.code == mezura_core::warnings::Code::UnknownLanguage {
            keep(warning);
        } else {
            emit(warning);
        }
    }
}

fn emitted() -> &'static Mutex<Vec<Warning>> {
    EMITTED.get_or_init(|| Mutex::new(Vec::new()))
}

// What a mezura user can do about it, added to the sentence the library wrote. The library cannot
// say it: a program counting through mezura-core has no data directory and no command line, and
// would be told to do something it cannot.
fn add_advice_to(mut warning: Warning) -> Warning {
    use mezura_core::warnings::Code;

    // Deliberately not exhaustive: a new code arriving without a line of advice is a missing nicety
    // and not a wrong answer.
    let advice = match warning.code {
        // The priority file settles extensions and filenames only. A contested shebang written into
        // it is skipped without a word, so sending its owner there would be advice that fails
        // silently.
        Code::LanguageTiebreak => Some(format!("A contested extension or filename is settled for good in '{}'; \
'--force-language {}=<language>' decides it for this run.",
                mezura_core::EXTENSION_PRIORITY_FILE_NAME, warning.subject)),
        Code::DuplicateLanguage => Some("Delete the copies you do not want from the 'languages' directory of your data directory.".to_owned()),
        Code::UnknownForcedLanguage => Some("Run with '--show-languages' for the ones available to '--force-language'.".to_owned()),
        Code::UnknownLanguage => Some("Run with '--show-languages' for the ones available to '--languages'.".to_owned()),
        Code::UnknownExcludedLanguage =>
                Some("Run with '--show-languages' for the ones available to '--exclude-languages'.".to_owned()),
        Code::UnknownSectionLanguage =>
                Some("Correct the 'Nested language default' line of that language file in your data dir.".to_owned()),
        _ => None
    };

    if let Some(advice) = advice {
        warning.message = format!("{}\n{advice}", warning.message);
    }
    warning
}

#[cfg(test)]
mod tests {
    use mezura_core::warnings::Code;

    use super::*;

    // The collector is shared by the whole process and the other tests run beside this one, so it
    // looks for its own warning rather than counting what is in there.
    #[test]
    fn a_kept_warning_reaches_the_collector() {
        keep(Warning::new(Code::LanguageTiebreak, "a-subject-no-other-test-uses",
                "the readable half".to_owned()));

        let mine = get_collected_warnings().into_iter().find(|x| x.subject == "a-subject-no-other-test-uses").unwrap();
        assert_eq!(Code::LanguageTiebreak, mine.code);
        assert!(mine.message.starts_with("the readable half"), "{}", mine.message);
    }

    #[test]
    fn the_advice_this_program_can_give_is_added_to_the_librarys_sentence() {
        let advised = add_advice_to(Warning::new(Code::LanguageTiebreak, "m",
                "The extension 'm' is claimed by MATLAB and Objective-C.".to_owned()));

        assert!(advised.message.starts_with("The extension 'm' is claimed by MATLAB and Objective-C.\n"),
                "the library's own sentence was not kept whole:\n{}", advised.message);
        assert!(advised.message.contains("--force-language m=<language>"), "{}", advised.message);
        assert!(advised.message.contains("extension_priority.txt"), "{}", advised.message);

        for (code, command) in [(Code::UnknownForcedLanguage, "--force-language"), (Code::UnknownLanguage, "--languages"),
                (Code::UnknownExcludedLanguage, "--exclude-languages")] {
            let advised = add_advice_to(Warning::new(code, "zz", "not found.".to_owned()));
            assert!(advised.message.contains(command), "{} did not name '{command}':\n{}", code.name(), advised.message);
        }

        let untouched = add_advice_to(Warning::new(Code::PriorityLineSkipped, "a line",
                "that line was skipped.".to_owned()));
        assert_eq!("that line was skipped.", untouched.message);
    }
}
