// Which languages a run has in play, and which of them owns an extension two of them claim. The
// format the definitions are written in is 'language_file' next door.
use std::collections::HashMap;

use std::sync::Arc;


use crate::{Language, warnings};
use crate::engine::config::EngineConfig;
use crate::engine::extensions::make_extension_language_map;
use crate::warnings::Warning;


// The one answer to "which languages exist, and which of them owns a contested extension".
//
// Resolved by the caller and not inside 'run', because working out that '--force-lang zz=Nope' names
// nothing, or that two languages both claim '.m', is a judgement about the settings: it belongs
// beside the other complaints about settings and not in the middle of a report. What comes back is a
// list of warnings, and whoever asked decides what to do with them.
pub struct Languages {
    definitions: HashMap<String, Language>,
    extension_map: HashMap<String, Arc<str>>
}

impl Languages {
    // The only way to build one, so the narrowing by '--languages' and the extension map can never
    // disagree about which languages are in play.
    pub fn resolve(definitions: HashMap<String, Language>, priority: &HashMap<String, Vec<String>>,
            config: &EngineConfig) -> (Self, Vec<Warning>)
    {
        let (definitions, mut reported) = retain_languages_of_interest(definitions, config);
        let (extension_map, report) = make_extension_language_map(&definitions, priority, &config.forced_languages);
        reported.extend(report.warnings());

        (Languages { definitions, extension_map }, reported)
    }

    pub(crate) fn into_parts(self) -> (HashMap<String, Language>, HashMap<String, Arc<str>>) {
        (self.definitions, self.extension_map)
    }
}

// The names that were asked for and do not exist as language files, in the order they were given.
pub fn unknown_language_names(definitions: &HashMap<String,Language>, wanted: &[String]) -> Vec<String> {
    wanted.iter().filter(|name| !definitions.keys().any(|x| x.eq_ignore_ascii_case(name)))
            .cloned().collect()
}

// Reported rather than printed: a name that does not exist is the caller's to complain about, and
// the command line has a suggested spelling to put next to it.
fn retain_languages_of_interest(mut definitions: HashMap<String,Language>, config: &EngineConfig)
        -> (HashMap<String,Language>, Vec<Warning>)
{
    let mut reported = Vec::new();
    if !config.languages_of_interest.is_empty() {
        for name in unknown_language_names(&definitions, &config.languages_of_interest) {
            reported.push(Warning::new(warnings::UNKNOWN_LANGUAGE, warnings::Affects::Settings, &name,
                    format!("'{name}' does not exist as a language file, so nothing was counted for it.")));
        }
        definitions.retain(|name, _| config.languages_of_interest.iter().any(|x| x.eq_ignore_ascii_case(name)));
    }

    for excluded in &config.excluded_languages {
        definitions.retain(|name, _| name.to_lowercase() != excluded.to_lowercase());
    }

    (definitions, reported)
}


#[cfg(test)]
mod language_selection_tests {
    use super::*;
    use crate::languages_claiming;

    // The command line reports a misspelling to a person; this is the half that decides what gets
    // counted, and it is what a library caller gets with no command line involved at all.
    #[test]
    fn the_run_narrows_the_languages_and_records_a_name_that_does_not_exist() {
        let languages = || languages_claiming(&[("Java", &["java"]), ("C#", &["cs"]), ("Rust", &["rs"])]);
        let names_of = |map: HashMap<String,Language>| {
            let mut names = map.into_keys().collect::<Vec<_>>();
            names.sort();
            names
        };

        let mut config = EngineConfig::new(vec!["./".to_owned()]);
        assert_eq!(vec!["C#", "Java", "Rust"], names_of(retain_languages_of_interest(languages(), &config).0));

        // asked for by a name that differs in case, which is still the same language
        config.set_languages_of_interest(vec!["java".to_owned(), "RUST".to_owned()]);
        assert_eq!(vec!["Java", "Rust"], names_of(retain_languages_of_interest(languages(), &config).0));

        // and the exclusion applies on top of the selection
        config.excluded_languages = vec!["rust".to_owned()];
        assert_eq!(vec!["Java"], names_of(retain_languages_of_interest(languages(), &config).0));

        // an excluded name on its own leaves everything else
        config.set_languages_of_interest(Vec::new());
        assert_eq!(vec!["C#", "Java"], names_of(retain_languages_of_interest(languages(), &config).0));

        assert_eq!(vec!["Erlang"], unknown_language_names(&languages(), &["java".to_owned(), "Erlang".to_owned()]));
        assert!(unknown_language_names(&languages(), &["C#".to_owned()]).is_empty());
    }

    // Returned and not printed, because the command line puts its own coloured version on the
    // screen with a suggested spelling next to it.
    #[test]
    fn a_language_that_does_not_exist_reaches_the_document_as_a_warning() {
        let mut config = EngineConfig::new(vec!["./".to_owned()]);
        config.set_languages_of_interest(vec!["Java".to_owned(), "Nolang-Q9".to_owned()]);
        let (_, reported) = retain_languages_of_interest(languages_claiming(&[("Java", &["java"])]), &config);

        let mine = reported.into_iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::UNKNOWN_LANGUAGE, mine.code);
        // the counts are sound for what does exist, it is the setting that was not honoured
        assert_eq!("settings", mine.affects.name());
    }
}
