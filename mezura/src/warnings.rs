// Warnings are values by the time they reach here. Collecting them for the document and putting them
// on the screen are both about showing, so both live on this side.
use std::sync::{Mutex, OnceLock};

use mezura_core::warnings::Warning;

// Held the way the active theme is, because warnings arrive from phases that share no value between
// them and some of them run before a configuration exists at all.
static EMITTED : OnceLock<Mutex<Vec<Warning>>> = OnceLock::new();

// Printed and kept in one call, so the terminal and the document cannot end up saying two different
// things about the same warning.
pub fn emit(warning: Warning) {
    let warning = with_advice(warning);
    eprintln!("\n{}", super::theme::active().warning.paint(&warning.message));
    emitted().lock().unwrap().push(warning);
}

// For the places that print in a shape of their own, and would say it twice if this printed too
pub fn keep(warning: Warning) {
    emitted().lock().unwrap().push(with_advice(warning));
}

pub fn collected() -> Vec<Warning> {
    emitted().lock().unwrap().clone()
}

fn emitted() -> &'static Mutex<Vec<Warning>> {
    EMITTED.get_or_init(|| Mutex::new(Vec::new()))
}

// What a mezura user can do about it, added to the sentence the library wrote. The library knows what
// happened but not that whoever called it has a data directory and a command line; a program counting
// through mezura-core has neither and would be told to do something it cannot.
//
// Applied on both ways into the collector, so a warning reads the same on the screen and in the
// document whichever door it came through.
fn with_advice(mut warning: Warning) -> Warning {
    use mezura_core::warnings::*;

    let advice = match warning.code {
        EXTENSION_TIEBREAK => Some(format!("Declare it in '{}', or run with '--force-lang {}=<language>'.",
                mezura_core::EXTENSION_PRIORITY_FILE_NAME, warning.subject)),
        DUPLICATE_LANGUAGE => Some("Delete the copies you do not want from the 'languages' folder of your data dir.".to_owned()),
        // Naming the command that caused it, since the reader typed one of three and the sentence
        // above says only which name was not found.
        UNKNOWN_FORCED_LANGUAGE => Some("Run with '--show-languages' for the ones available to '--force-lang'.".to_owned()),
        UNKNOWN_LANGUAGE => Some("Run with '--show-languages' for the ones available to '--languages'.".to_owned()),
        UNKNOWN_EXCLUDED_LANGUAGE =>
                Some("Run with '--show-languages' for the ones available to '--exclude-languages'.".to_owned()),
        _ => None
    };

    if let Some(advice) = advice {
        warning.message = format!("{}\n{advice}", warning.message);
    }
    warning
}

#[cfg(test)]
mod tests {
    use mezura_core::warnings::{Affects, EXTENSION_TIEBREAK, PRIORITY_LINE_SKIPPED, UNKNOWN_EXCLUDED_LANGUAGE,
            UNKNOWN_FORCED_LANGUAGE, UNKNOWN_LANGUAGE};

    use super::*;

    // The collector is shared by the whole process, so this asks whether its own warning arrived
    // rather than counting what is in there: the other tests of this binary run beside it and keep
    // their own.
    #[test]
    fn a_kept_warning_reaches_the_collector() {
        keep(Warning::new(EXTENSION_TIEBREAK, Affects::Counts, "a-subject-no-other-test-uses",
                "the readable half".to_owned()));

        let mine = collected().into_iter().find(|x| x.subject == "a-subject-no-other-test-uses").unwrap();
        assert_eq!(EXTENSION_TIEBREAK, mine.code);
        assert!(mine.message.starts_with("the readable half"), "{}", mine.message);
    }

    // The library says what happened and this side says what to do about it, because only this side
    // has a data directory and a command line. A caller using mezura-core on its own gets the first
    // half and is not told to edit a file it does not have.
    #[test]
    fn the_advice_this_program_can_give_is_added_to_the_librarys_sentence() {
        let advised = with_advice(Warning::new(EXTENSION_TIEBREAK, Affects::Counts, "m",
                "The extension 'm' is claimed by MATLAB and Objective-C.".to_owned()));

        assert!(advised.message.starts_with("The extension 'm' is claimed by MATLAB and Objective-C.\n"),
                "the library's own sentence was not kept whole:\n{}", advised.message);
        // naming the extension it is about, so the line can be typed as it stands
        assert!(advised.message.contains("--force-lang m=<language>"), "{}", advised.message);
        assert!(advised.message.contains("extension_priority.txt"), "{}", advised.message);

        // Each of the three unknown-name warnings names the command that caused it, since the
        // sentence above says only which name was not found and the reader typed one of three.
        for (code, command) in [(UNKNOWN_FORCED_LANGUAGE, "--force-lang"), (UNKNOWN_LANGUAGE, "--languages"),
                (UNKNOWN_EXCLUDED_LANGUAGE, "--exclude-languages")] {
            let advised = with_advice(Warning::new(code, Affects::Settings, "zz", "not found.".to_owned()));
            assert!(advised.message.contains(command), "{code} did not name '{command}':\n{}", advised.message);
        }

        // A warning this program has no advice for is passed through untouched, rather than given
        // something vague to keep the shape
        let untouched = with_advice(Warning::new(PRIORITY_LINE_SKIPPED, Affects::Settings, "a line",
                "that line was skipped.".to_owned()));
        assert_eq!("that line was skipped.", untouched.message);
    }
}
