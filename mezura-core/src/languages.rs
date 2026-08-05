// Which languages a run has in play, and which of them owns an extension two of them claim. The
// format a language file is written in is 'language_file' next door.
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
    by_name: HashMap<String, Language>,
    extension_map: HashMap<String, Arc<str>>,
    // Which configuration produced this set, so that 'run' can refuse one that would have produced
    // a different one. See 'LanguageSelection' below.
    resolved_against: LanguageSelection
}

// The whole of what resolution reads from a configuration: which languages were asked for, which
// were excluded, and which extensions were forced. Nothing else about a configuration can change
// the set that comes out, which is why 'dirs' is not here and a caller may resolve once and then
// count several directories with the same 'Languages'.
//
// Normalised the way the matching normalises: order does not matter and neither does case, so two
// configurations that would have produced this same set compare equal and no honest run is refused.
#[derive(PartialEq, Eq, Debug)]
struct LanguageSelection {
    of_interest: Vec<String>,
    excluded: Vec<String>,
    forced: HashMap<String, String>
}

impl LanguageSelection {
    fn of(config: &EngineConfig) -> Self {
        let folded = |names: &[String]| {
            let mut names = names.iter().map(|x| x.to_lowercase()).collect::<Vec<_>>();
            names.sort();
            names
        };

        LanguageSelection {
            of_interest: folded(&config.languages_of_interest),
            excluded: folded(&config.excluded_languages),
            forced: config.forced_languages.iter().map(|(extension, language)|
                    (crate::engine::extensions::extension_key(extension), language.to_lowercase())).collect()
        }
    }
}

impl Languages {
    // What a caller who wants what mezura counts by default writes, and the whole of it. The
    // languages are the ones baked into this crate, so nothing on the machine is read and nothing
    // an installation has been given is seen: the command line reads its own directory on purpose,
    // because a language file there is the user's to edit.
    pub fn shipped(config: &EngineConfig) -> (Self, Vec<Warning>) {
        Self::resolve(config, shipped_languages(), &shipped_extension_priority())
    }

    // The door for a caller with languages of its own, whether that is ours with one more added,
    // a directory of its own, or a set that has nothing to do with the shipped one. The only way to
    // build one either way, so the narrowing by '--languages' and the extension map can never
    // disagree about which languages are in play.
    //
    // Keyed here, by each language's own name, rather than taken as a map somebody else keyed: a map
    // whose key says one thing and whose value says another has a winner, and it was never the
    // value, so a language would be counted and reported under a name it does not carry.
    pub fn resolve(config: &EngineConfig, languages: impl IntoIterator<Item = Language>,
            priority: &HashMap<String, Vec<String>>) -> (Self, Vec<Warning>)
    {
        // Unusable ones go first, so that a name nobody can ask for is not in the list when the
        // narrowing below asks whether a name exists. Duplicates are reported last, after the
        // narrowing, so a run that never asked for the language is not told about it.
        let (languages, mut reported) = drop_the_unusable(languages.into_iter().collect());
        let (languages, narrowing) = retain_languages_of_interest(languages, config);
        reported.extend(narrowing);
        reported.extend(duplicate_names(&languages));

        let by_name = keyed_by_name(languages);
        let (extension_map, report) = make_extension_language_map(&by_name, priority, &config.forced_languages);
        reported.extend(report.warnings());

        (Languages { by_name, extension_map, resolved_against: LanguageSelection::of(config) }, reported)
    }

    // Whether this set is the one the given configuration describes. 'run' asks before it counts
    // anything, because the alternative is the worst answer a counter can give: resolving against a
    // configuration naming Rust and then running with one naming Python counted Rust, called it
    // Rust, and said nothing, and deriving one configuration from another is exactly what the
    // struct update syntax on 'EngineConfig' exists for.
    pub(crate) fn describe_the_same_selection_as(&self, config: &EngineConfig) -> bool {
        self.resolved_against == LanguageSelection::of(config)
    }

    pub(crate) fn into_parts(self) -> (HashMap<String, Language>, HashMap<String, Arc<str>>) {
        (self.by_name, self.extension_map)
    }
}

// What this crate ships, in the two shapes anyone wants it in. The '_raw' pair is the bytes exactly
// as they are written, for whoever installs the files rather than counts with them: the command line
// writes those to disk unchanged and parses one at a time to ask whether a user's copy still means
// what ours means. Everybody else wants the pair above, already parsed. A caller who wants nothing
// but the default has 'Languages::shipped' and needs none of the four.

// Ready to be handed to 'resolve', or to be added to first.
pub fn shipped_languages() -> Vec<Language> {
    // These are ours and every one of them parses, which 'every_shipped_language_file_parses' is
    // what actually guarantees. One that somehow did not would be left out rather than panic here.
    shipped_language_files_raw().into_iter()
            .filter_map(|(_, contents)| crate::language_file::parse_language(&String::from_utf8_lossy(contents)))
            .collect()
}

// The rule this crate ships for settling an extension that two languages both claim.
pub fn shipped_extension_priority() -> HashMap<String, Vec<String>> {
    crate::language_file::parse_priority(&String::from_utf8_lossy(shipped_extension_priority_raw())).0
}

// The two below are for writing these files out to somebody's disk, which is what lets them edit a
// language, add one of their own, or change which language wins a contested extension. Whoever
// wants to *count* wants 'shipped_languages' and 'shipped_extension_priority' above, which hand back
// parsed values; these hand back the bytes as they were authored, comments and layout included, so
// that what lands in the user's folder is a file made to be read and edited rather than something
// re-serialised from a struct.
//
// They exist as public because the program that installs them is a separate crate and cannot reach
// into this one's 'data/' directory. Without them it would keep a second copy of every language
// file, which is the thing the split was done to avoid.
//
// The file name travels with the bytes because the installer writes them under it. Plain tuples and
// not the embedder's own file type, so that a release of 'include_dir' is never a breaking change
// of ours.
pub fn shipped_language_files_raw() -> Vec<(&'static str, &'static [u8])> {
    include_dir::include_dir!("data/languages").files.iter()
            .map(|file| (std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path),
                    file.contents))
            .collect()
}

pub fn shipped_extension_priority_raw() -> &'static [u8] {
    include_bytes!("../data/extension_priority.txt")
}

// The names that were asked for and do not exist as language files, in the order they were given.
pub fn unknown_language_names(languages: &[Language], wanted: &[String]) -> Vec<String> {
    wanted.iter().filter(|name| !languages.iter().any(|x| is_the_same_language_name(&x.name, name)))
            .cloned().collect()
}

// The one rule for whether two spellings name the same language, and every place that matches a
// name goes through it: the selection, the exclusion, '--force-lang' and the priority file.
//
// They used to disagree. The exclusion folded case with 'to_lowercase' and everything else with
// 'eq_ignore_ascii_case', which are the same answer until a name carries a letter outside ASCII:
// a language called 'CAFÉ' excluded as 'café' was dropped from the count by the first and reported
// as a name that does not exist by the second, in the same run. The reader is then told the flag
// did nothing while it was quietly removing a whole language from the total.
pub(crate) fn is_the_same_language_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

// By the name each language carries. A later declaration of a name wins, which is what a directory
// holding two files for one language has always done.
pub(crate) fn keyed_by_name(languages: impl IntoIterator<Item = Language>) -> HashMap<String, Language> {
    languages.into_iter().map(|language| (language.name.clone(), language)).collect()
}

// A language that can never match a file and can never be named. Both were accepted in silence:
// they took a row in every internal map, contributed nothing, and an empty name went on to answer
// to a '--force-lang' whose language was left blank. The file parser refuses both already, so what
// this catches is a caller building one by hand.
fn drop_the_unusable(languages: Vec<Language>) -> (Vec<Language>, Vec<Warning>) {
    let mut reported = Vec::new();
    let kept = languages.into_iter().filter(|language| {
        if language.name.trim().is_empty() {
            reported.push(Warning::new(warnings::UNUSABLE_LANGUAGE, warnings::Affects::Settings,
                    &language.extensions.join(","),
                    format!("A language claiming '{}' has no name, so nothing could be counted or reported for it.",
                    language.extensions.join(", "))));
            return false;
        }
        if language.extensions.is_empty() {
            reported.push(Warning::new(warnings::UNUSABLE_LANGUAGE, warnings::Affects::Settings, &language.name,
                    format!("'{}' claims no extension, so no file can ever be counted as it.", language.name)));
            return false;
        }
        true
    }).collect();

    (kept, reported)
}

// Two definitions of one name: the second silently replaces the first when the map is built, and
// which one is second is whatever order the directory was read in, so renaming a file changes the
// counts. Reported against the counts and not the settings, because that is what it changes: the
// two definitions disagree about comment symbols, and the losing one takes its extensions out of
// the run with it. Announcing it is the fix; picking a winner would mean inventing a rule for
// which of two files somebody meant.
// Grouped the way every other name comparison in this crate groups, through
// 'is_the_same_language_name', and not by the exact spelling. Two files called 'Rust' and 'rust' are
// two definitions of one language to '--languages', to '--exclude-languages', to '--force-lang' and
// to the priority file, all of which fold case; counting them apart here was the one place that did
// not, so that pair went through unreported while every one of those flags treated them as one.
fn duplicate_names(languages: &[Language]) -> Vec<Warning> {
    let mut spellings : HashMap<String, Vec<&str>> = HashMap::new();
    for language in languages {
        spellings.entry(language.name.to_lowercase()).or_default().push(language.name.as_str());
    }

    let mut duplicated = spellings.into_values().filter(|found| found.len() > 1).collect::<Vec<_>>();
    duplicated.sort();

    duplicated.into_iter().map(|found| {
        let times = found.len();
        let name = found[0];
        // The two cases behave differently and the sentence has to say which one this is. Identical
        // spellings collapse into one entry of the map and one of them is simply gone; spellings
        // that differ in case are separate entries that both survive, take a row each in the report
        // and split the count of one language between them.
        let detail = if found.iter().all(|other| *other == name) {
            format!("'{name}' is declared {times} times, and only one of those declarations was used. \
Which one is not decided by anything you can see, so the counts of '{name}' depend on it.")
        } else {
            format!("'{}' are {times} spellings of one name, so they are one language to every command \
that takes one and {times} languages in the report, each counting part of the files.",
                    found.join("' and '"))
        };
        Warning::new(warnings::DUPLICATE_LANGUAGE, warnings::Affects::Counts, name,
                format!("{detail} Delete the copies you do not want from the languages folder."))
    }).collect()
}

// Reported rather than printed: a name that does not exist is the caller's to complain about, and
// the command line has a suggested spelling to put next to it.
fn retain_languages_of_interest(mut languages: Vec<Language>, config: &EngineConfig)
        -> (Vec<Language>, Vec<Warning>)
{
    let mut reported = Vec::new();
    if !config.languages_of_interest.is_empty() {
        for name in unknown_language_names(&languages, &config.languages_of_interest) {
            reported.push(Warning::new(warnings::UNKNOWN_LANGUAGE, warnings::Affects::Settings, &name,
                    format!("'{name}' does not exist as a language file, so nothing was counted for it.")));
        }
    }

    // Asked of the whole list and not of what the selection above left of it. Checked after the
    // narrowing, every excluded name that happened to be outside the selection was reported as a
    // name that does not exist, which is every excluded name on any run that also names languages:
    // '--languages Java --exclude-languages Rust' said Rust did not exist.
    //
    // Under a code of its own, and not the one above it, because the command line has already put
    // that one on the screen with a suggested spelling and keeps it only for the document. This one
    // has no other voice.
    for name in unknown_language_names(&languages, &config.excluded_languages) {
        reported.push(Warning::new(warnings::UNKNOWN_EXCLUDED_LANGUAGE, warnings::Affects::Settings, &name,
                format!("'{name}' does not exist as a language file, so excluding it changed nothing.")));
    }

    if !config.languages_of_interest.is_empty() {
        languages.retain(|language| config.languages_of_interest.iter()
                .any(|x| is_the_same_language_name(x, &language.name)));
    }
    languages.retain(|language| !config.excluded_languages.iter()
            .any(|x| is_the_same_language_name(x, &language.name)));

    (languages, reported)
}


#[cfg(test)]
mod language_selection_tests {
    use super::*;
    use crate::languages_claiming;

    // The command line reports a misspelling to a person; this is the half that decides what gets
    // counted, and it is what a library caller gets with no command line involved at all.
    #[test]
    fn the_run_narrows_the_languages_and_records_a_name_that_does_not_exist() {
        let languages = || languages_claiming(&[("Java", &["java"]), ("C#", &["cs"]), ("Rust", &["rs"])])
                .into_values().collect::<Vec<_>>();
        let names_of = |languages: Vec<Language>| {
            let mut names = languages.into_iter().map(|x| x.name).collect::<Vec<_>>();
            names.sort();
            names
        };

        let mut config = EngineConfig::default();
        assert_eq!(vec!["C#", "Java", "Rust"], names_of(retain_languages_of_interest(languages(), &config).0));

        // asked for by a name that differs in case, which is still the same language
        config.languages_of_interest = vec!["java".to_owned(), "RUST".to_owned()];
        assert_eq!(vec!["Java", "Rust"], names_of(retain_languages_of_interest(languages(), &config).0));

        // and the exclusion applies on top of the selection
        config.excluded_languages = vec!["rust".to_owned()];
        assert_eq!(vec!["Java"], names_of(retain_languages_of_interest(languages(), &config).0));

        // an excluded name on its own leaves everything else
        config.languages_of_interest = Vec::new();
        assert_eq!(vec!["C#", "Java"], names_of(retain_languages_of_interest(languages(), &config).0));

        assert_eq!(vec!["Erlang"], unknown_language_names(&languages(), &["java".to_owned(), "Erlang".to_owned()]));
        assert!(unknown_language_names(&languages(), &["C#".to_owned()]).is_empty());
    }

    // Returned and not printed, because the command line puts its own coloured version on the
    // screen with a suggested spelling next to it.
    #[test]
    fn a_language_that_does_not_exist_reaches_the_document_as_a_warning() {
        let config = EngineConfig {
            languages_of_interest: vec!["Java".to_owned(), "Nolang-Q9".to_owned()],
            ..Default::default()
        };
        let (_, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"])]).into_values().collect(), &config);

        let mine = reported.into_iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::UNKNOWN_LANGUAGE, mine.code);
        // the counts are sound for what does exist, it is the setting that was not honoured
        assert_eq!("settings", mine.affects.name());
    }

    // The names travel with the bytes because the command line writes each file to disk under the
    // name it came with, prefixed by its folder. Returning the whole embedded path instead passed
    // every test there was, and would have installed 'languages/data/languages/Rust.txt'.
    #[test]
    fn the_shipped_files_carry_bare_names_and_match_the_directory() {
        let raw = shipped_language_files_raw();
        assert!(!raw.is_empty());
        for (name, contents) in &raw {
            assert!(!name.contains('/') && !name.contains('\\'), "'{name}' is a path and not a file name");
            assert!(name.ends_with(".txt"), "'{name}' is not a language file name");
            assert!(!contents.is_empty(), "'{name}' is empty");
        }

        let mut shipped = raw.iter().map(|(name, _)| (*name).to_owned()).collect::<Vec<_>>();
        let mut on_disk = std::fs::read_dir(crate::test_paths::LANGUAGES_DIR).unwrap()
                .flatten().filter(|x| x.path().is_file())
                .map(|x| x.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        shipped.sort();
        on_disk.sort();
        assert_eq!(on_disk, shipped, "the embedded set and the directory have drifted apart");
    }

    // The exclusion folded case one way and the check for whether the name exists folded it another,
    // so a name with a letter outside ASCII went down both roads at once: removed from the count,
    // and reported as a name that removing changed nothing about.
    #[test]
    fn a_language_name_outside_ascii_is_excluded_and_not_reported_as_missing() {
        let cafe = || vec![Language::new("CAFÉ", ["cf"], ["\""], ["//"], None, []),
                Language::new("Rust", ["rs"], ["\""], ["//"], None, [])];
        let names_of = |languages: Vec<Language>| languages.into_iter().map(|x| x.name).collect::<Vec<_>>();

        let config = EngineConfig { excluded_languages: vec!["café".to_owned()], ..Default::default() };
        let (kept, reported) = retain_languages_of_interest(cafe(), &config);

        assert_eq!(vec!["Rust"], names_of(kept), "the accented name survived an exclusion that names it");
        assert!(reported.is_empty(), "the language was excluded and the run said it does not exist: {reported:?}");

        // and the selection folds case the same way, so asking for it by the other spelling finds it
        let config = EngineConfig { languages_of_interest: vec!["café".to_owned()], ..Default::default() };
        let (kept, reported) = retain_languages_of_interest(cafe(), &config);
        assert_eq!(vec!["CAFÉ"], names_of(kept));
        assert!(reported.is_empty(), "{reported:?}");
    }

    // The excluded names were checked against what the selection had already left behind, so any
    // excluded name outside the selection was reported as a name that does not exist. That is every
    // excluded name on any run that also names languages, which teaches the reader to ignore the code.
    #[test]
    fn excluding_a_language_outside_the_selection_is_not_reported_as_missing() {
        let config = EngineConfig {
            languages_of_interest: vec!["Java".to_owned()],
            excluded_languages: vec!["Rust".to_owned()],
            ..Default::default()
        };
        let (kept, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])]).into_values().collect(), &config);

        assert_eq!(vec!["Java"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert!(!reported.iter().any(|x| x.subject == "Rust"),
                "'Rust' exists and was reported as missing: {reported:?}");
    }

    // Two definitions of one name: the map keeps the last and the order is the directory's, so
    // renaming a file changes which comment symbols a language counts with. Measured before this:
    // the same source came back as comment=0 code=3 or comment=4 code=1 depending on the file names.
    #[test]
    fn two_definitions_of_one_name_are_reported_against_the_counts() {
        let twice = vec![Language::new("Same", ["aa"], ["\""], ["//"], None, []),
                Language::new("Same", ["bb"], ["\""], [""; 0], Some(("/*", "*/")), []),
                Language::new("Rust", ["rs"], ["\""], ["//"], None, [])];

        let config = EngineConfig::default();
        let (languages, _) = Languages::resolve(&config, twice, &HashMap::new());
        let reported = Languages::resolve(&config,
                vec![Language::new("Same", ["aa"], ["\""], ["//"], None, []),
                     Language::new("Same", ["bb"], ["\""], [""; 0], Some(("/*", "*/")), [])],
                &HashMap::new()).1;

        let mine = reported.iter().find(|x| x.code == warnings::DUPLICATE_LANGUAGE)
                .expect("a language declared twice was dropped in silence");
        assert_eq!("Same", mine.subject);
        assert_eq!("counts", mine.affects.name(), "the choice changes numbers, not settings");
        // one of the two really is gone, which is what the warning is about
        assert_eq!(2, languages.into_parts().0.len());
    }

    // A language nobody can name and a language no file can match were both accepted, took a row in
    // every internal map, and contributed nothing. The file parser refuses both, so this is the
    // caller who built one by hand.
    #[test]
    fn a_language_that_cannot_be_named_or_matched_is_dropped_and_reported() {
        let unusable = vec![
            Language::new("   ", ["zz"], ["\""], ["//"], None, []),
            Language::new("Nameless-Extensions", [""; 0], ["\""], ["//"], None, []),
            Language::new("Rust", ["rs"], ["\""], ["//"], None, [])];

        let (kept, reported) = drop_the_unusable(unusable);
        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert_eq!(2, reported.len(), "{reported:?}");
        assert!(reported.iter().all(|x| x.code == warnings::UNUSABLE_LANGUAGE));
        assert!(reported.iter().any(|x| x.subject == "zz"), "the nameless one is named by what it claims");
        assert!(reported.iter().any(|x| x.subject == "Nameless-Extensions"));
    }

    // Excluding a name that does not exist did nothing and said nothing, while asking for one four
    // lines above it has always been reported. Its own code, because the command line prints this one
    // and only keeps the other.
    #[test]
    fn excluding_a_language_that_does_not_exist_is_reported_too() {
        let config = EngineConfig {
            excluded_languages: vec!["Java".to_owned(), "Nolang-Q9".to_owned()],
            ..Default::default()
        };
        let (kept, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])]).into_values().collect(), &config);

        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        let mine = reported.iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::UNKNOWN_EXCLUDED_LANGUAGE, mine.code);
        assert_eq!("settings", mine.affects.name());
        // and the one that does exist is excluded without a word about it
        assert!(!reported.iter().any(|x| x.subject == "Java"));
    }
}
