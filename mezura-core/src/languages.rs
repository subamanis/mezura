// Which languages a run has in play, and which of them owns an extension two of them claim. The
// format a language file is written in is 'language_file' next door.
use std::{collections::HashMap, sync::Arc};

use crate::{Language, warnings};
use crate::engine::config::EngineConfig;
use crate::engine::identity::{IdentifiedBy, LanguageLookup, build_language_map_by, extension_key};
use crate::language_file::PriorityRules;
use crate::warnings::Warning;

// Built by the caller and handed to 'run', rather than inside it, so that its complaints about the
// settings land with the other complaints about settings and not in the middle of a report.
pub struct Languages {
    by_name: HashMap<String, Language>,
    lookup: LanguageLookup,
    nested: NestedLanguageDefinitions,
    // Which settings produced this set, so 'run' can refuse one that would have produced another.
    resolved_against: LanguageSelection
}

impl Languages {
    // The languages baked into this crate, so nothing on the machine is read. The command line reads
    // its own folder instead, because a language file there is the user's to edit.
    pub fn shipped(config: &EngineConfig) -> (Self, Vec<Warning>) {
        Self::resolve(config, parse_shipped_languages(), &parse_shipped_extension_priority())
    }

    // For a caller with languages of its own. Keyed here by each language's own name rather than
    // taken as a map somebody else keyed: in a map whose key and value disagree the key wins, and a
    // language would be counted under a name it does not carry.
    pub fn resolve(config: &EngineConfig, languages: impl IntoIterator<Item = Language>,
            priority: &PriorityRules) -> (Self, Vec<Warning>)
    {
        // Unusable ones go first, so that a name nobody can ask for is not in the list when the
        // narrowing below asks whether a name exists. Duplicates are reported last, after the
        // narrowing, so a run that never asked for the language is not told about it.
        let (languages, mut reported) = drop_the_unusable(languages.into_iter().collect());
        // Over everything, before the narrowing, because it answers two questions the narrowed set
        // cannot: which language an extension names when '--languages' is given one, and what a
        // section inside a container file is written in when the run narrowed that language away.
        // Its own complaints about contested extensions are dropped, since the narrowed build below
        // makes them, and a contest between two languages the run then leaves out is not news.
        let (all_extensions, _) = build_language_map_by(IdentifiedBy::Extension, &keyed_by_name(languages.clone()),
                &priority.by_extension, &config.forced_languages);
        let (languages, set_aside, narrowing) = retain_languages_of_interest(languages, &all_extensions, config);
        reported.extend(narrowing);
        reported.extend(find_duplicate_names(&languages));

        let by_name = keyed_by_name(languages);
        let (by_extension, report) = build_language_map_by(IdentifiedBy::Extension, &by_name, &priority.by_extension,
                &config.forced_languages);
        reported.extend(report.collect_warnings());
        // The forced pairs go to both, since '--force-language Makefile=python' and '--force-language
        // txt=python' are the same sentence and the reader has no reason to know which map answers
        let (by_filename, filename_report) = build_language_map_by(IdentifiedBy::Filename, &by_name,
                &priority.by_filename, &config.forced_languages);
        reported.extend(filename_report.collect_warnings());
        // The priority file has no block for a contested interpreter yet; the forced pairs reach
        // this map like the other two, which is how such a contest would be settled by hand.
        let (by_shebang, shebang_report) = build_language_map_by(IdentifiedBy::Shebang, &by_name,
                &HashMap::new(), &config.forced_languages);
        reported.extend(shebang_report.collect_warnings());
        reported.extend(find_unknown_forced_languages(&by_name, &config.forced_languages));

        // A section names its language whatever the run narrowed itself to, which is why the
        // languages the narrowing set aside are kept and the map handed over is the one built over
        // everything. Only carried when a language in play declares regions, so an ordinary run
        // holds no second copy of anything.
        let mut nested = NestedLanguageDefinitions::default();
        let set_aside = keyed_by_name(set_aside);
        if by_name.values().chain(set_aside.values()).any(|language| !language.nested_languages.is_empty()) {
            reported.extend(find_unresolvable_region_defaults(&by_name, &set_aside, &all_extensions));
            nested = NestedLanguageDefinitions { set_aside, extension_to_name: all_extensions };
        }

        (Languages { by_name, lookup: LanguageLookup { by_extension, by_filename, by_shebang },
                nested, resolved_against: LanguageSelection::of(config) }, reported)
    }

    // Asked by 'run' before it counts anything. Resolved against settings naming Rust and then run
    // with settings naming Python, it counted Rust, called it Rust, and said nothing.
    pub(crate) fn describe_the_same_selection_as(&self, config: &EngineConfig) -> bool {
        self.resolved_against == LanguageSelection::of(config)
    }

    pub(crate) fn into_parts(self) -> (HashMap<String, Language>, LanguageLookup, NestedLanguageDefinitions) {
        (self.by_name, self.lookup, self.nested)
    }
}

// What a section of another language resolves against: the whole set's extension map, and the
// definitions the narrowing took out of play. Empty on any run where no language declares regions.
#[derive(Default)]
pub(crate) struct NestedLanguageDefinitions {
    pub set_aside: HashMap<String, Language>,
    pub extension_to_name: HashMap<String, std::sync::Arc<str>>,
}

// What this crate ships, parsed for counting and raw for installing. A caller that wants nothing but
// the default has 'Languages::shipped' and needs none of the four.

pub fn parse_shipped_languages() -> Vec<Language> {
    // 'every_shipped_language_file_parses' is what guarantees these all parse. One that somehow did
    // not would be left out rather than panic here.
    get_shipped_language_files_raw().into_iter()
            .filter_map(|(_, contents)| crate::language_file::parse_language(&String::from_utf8_lossy(contents)))
            .collect()
}

// The rule this crate ships for settling an extension that two languages both claim.
pub fn parse_shipped_extension_priority() -> PriorityRules {
    crate::language_file::parse_priority(&String::from_utf8_lossy(get_shipped_extension_priority_raw())).0
}

// The bytes as they were authored, comments and layout included, so what the installer puts in the
// user's folder is a file made to be read and edited. Public because that installer is a separate
// crate and cannot reach into this one's 'data/'.
//
// Plain tuples and not the embedder's own file type, so a release of 'include_dir' is never a
// breaking change of ours.
pub fn get_shipped_language_files_raw() -> Vec<(&'static str, &'static [u8])> {
    include_dir::include_dir!("data/languages").files.iter()
            .map(|file| (std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path),
                    file.contents))
            .collect()
}

pub fn get_shipped_extension_priority_raw() -> &'static [u8] {
    include_bytes!("../data/extension_priority.txt")
}

// The names that were asked for and no language answers to, in the order they were given. A
// language answers to the name it carries and to every extension it claims, so 'js' is not an
// unknown name while some language counts '.js' files. Which language an extension belongs to when
// two claim it is a different question, settled where the narrowing happens; existence is not
// contested, so this needs no priority rules and no map.
pub fn find_unknown_language_names(languages: &[Language], wanted: &[String]) -> Vec<String> {
    wanted.iter().filter(|wanted| !languages.iter().any(|language|
                    is_the_same_language_name(&language.name, wanted)
                    || language.extensions.iter().any(|extension| is_the_same_language_name(extension, wanted))))
            .cloned().collect()
}

// Every place that matches a name goes through this one: choosing, excluding, forcing an extension,
// and the priority rules.
//
// 'to_lowercase' and not 'eq_ignore_ascii_case', which agree until a name has a letter outside ASCII:
// with the two mixed, 'CAFÉ' excluded as 'café' was taken out of the count by one rule and reported
// as a name that does not exist by the other, in the same run.
pub(crate) fn is_the_same_language_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

// By the name each language carries. A later declaration of a name wins, which is what a directory
// holding two files for one language has always done.
pub(crate) fn keyed_by_name(languages: impl IntoIterator<Item = Language>) -> HashMap<String, Language> {
    languages.into_iter().map(|language| (language.name.clone(), language)).collect()
}

// The whole of what building a 'Languages' reads from the settings. Nothing else can change the set
// that comes out, which is why the directories are not here and one 'Languages' counts several.
//
// Normalised the way the matching is: neither order nor case matters, so two settings that would
// have produced this same set compare equal and no honest run is refused.
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
                    (crate::engine::identity::extension_key(extension), language.to_lowercase())).collect()
        }
    }
}

// Asked once, and not inside the map building, because that runs once per kind of identity and is
// handed the same pairs every time: whoever writes '--force-language Makefile=python' owes us no
// difference between a name and an extension, so the answer must not depend on which map could use
// the pair.
fn find_unknown_forced_languages(by_name: &HashMap<String, Language>, forced: &HashMap<String,String>)
        -> Vec<Warning>
{
    let mut names = by_name.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();

    let mut unknown = forced.iter()
            .filter(|(_, wanted)| crate::engine::identity::find_language_named(&names, wanted).is_none())
            .collect::<Vec<_>>();
    unknown.sort();

    unknown.into_iter().map(|(claimed, wanted)| Warning::new(warnings::Code::UnknownForcedLanguage, claimed,
            format!("Nothing called '{wanted}' is among the languages in use, so '{claimed}' was left as it was.")))
            .collect()
}

// A language that can never match a file, or can never be named. The file parser refuses both, so
// what this catches is a caller building one by hand.
fn drop_the_unusable(languages: Vec<Language>) -> (Vec<Language>, Vec<Warning>) {
    let mut reported = Vec::new();
    let kept = languages.into_iter().filter(|language| {
        if language.name.trim().is_empty() {
            let claimed = language.extensions.iter().chain(&language.filenames)
                    .map(String::as_str).collect::<Vec<_>>();
            reported.push(Warning::new(warnings::Code::LanguageWithoutName, &claimed.join(","),
                    format!("A language claiming '{}' has no name, so the files matching it were not counted.",
                    claimed.join(", "))));
            return false;
        }
        if language.extensions.is_empty() && language.filenames.is_empty() && language.shebangs.is_empty() {
            reported.push(Warning::new(warnings::Code::LanguageClaimsNothing, &language.name,
                    format!("'{}' claims no extension, no filename and no shebang, so no file can ever be counted as it.",
                    language.name)));
            return false;
        }
        true
    }).collect();

    (kept, reported)
}

// A region's default names the language its sections fall to when their own tag names none, and
// nothing at the time a language file is read can tell whether that name exists, since the file
// knows only itself. Asked here, where every language and the extension map are in one place, and
// asked against exactly the two the section lookup will consult, so the check and what it predicts
// cannot drift. Left unreported it is silent and expensive: every unnamed section goes back to
// being read with the container's own symbols, so a '//' inside a script block counts as code.
fn find_unresolvable_region_defaults(by_name: &HashMap<String, Language>,
    set_aside: &HashMap<String, Language>, extensions: &HashMap<String, Arc<str>>) -> Vec<Warning>
{
    let resolves = |spelling: &str| {
        let lowered = spelling.to_lowercase();
        extensions.contains_key(&lowered)
                || by_name.values().chain(set_aside.values())
                        .any(|language| is_the_same_language_name(&language.name, spelling))
    };

    let mut unresolvable = by_name.values().chain(set_aside.values())
            .flat_map(|language| language.nested_languages.iter()
                    .map(move |region| (language.name.as_str(), region.default.as_str())))
            .filter(|(_, default)| !resolves(default))
            .collect::<Vec<_>>();
    unresolvable.sort();
    unresolvable.dedup();

    unresolvable.into_iter().map(|(language, default)| Warning::new(warnings::Code::UnknownSectionLanguage, default,
            format!("'{language}' says its sections fall to '{default}', and no language answers to that name or \
claims it as an extension. Those sections are counted with the symbols of '{language}' instead."))).collect()
}

// Two definitions of one name: the second silently replaces the first when the map is built, and
// which one is second is whatever order the directory was read in, so renaming a file changes the
// counts. Reported against the counts and not the settings, because that is what it changes: the
// two definitions disagree about comment symbols, and the losing one takes its extensions out of
// the run with it. Announcing it is the fix; picking a winner would mean inventing a rule for
// which of two files somebody meant.
// Grouped the way every other name comparison in this crate groups, through
// 'is_the_same_language_name', and not by the exact spelling. Two files called 'Rust' and 'rust' are
// two definitions of one language to '--languages', to '--exclude-languages', to '--force-language' and
// to the priority file, all of which fold case; counting them apart here was the one place that did
// not, so that pair went through unreported while every one of those flags treated them as one.
pub fn find_duplicate_names(languages: &[Language]) -> Vec<Warning> {
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
        Warning::new(warnings::Code::DuplicateLanguage, name, detail)
    }).collect()
}

// Reported rather than printed: a name that does not exist is the caller's to complain about, and
// the command line has a suggested spelling to put next to it.
fn retain_languages_of_interest(languages: Vec<Language>, extensions: &HashMap<String, Arc<str>>,
        config: &EngineConfig) -> (Vec<Language>, Vec<Language>, Vec<Warning>)
{
    // A spelling selects a language by the name it carries, or by an extension it owns. The
    // ownership is read from the map the counting itself uses, so '--languages m' means the same
    // language that every '.m' file is counted as, whether that was settled by the priority file,
    // by '--force-language' or by the tiebreak. Deciding it here again would let one word select
    // one language and count another.
    let selects = |spelling: &String, language: &Language| {
        is_the_same_language_name(&language.name, spelling)
                || extensions.get(&extension_key(spelling))
                        .is_some_and(|owner| owner.as_ref() == language.name)
    };
    let mut reported = Vec::new();
    if !config.languages_of_interest.is_empty() {
        for name in find_unknown_language_names(&languages, &config.languages_of_interest) {
            reported.push(Warning::new(warnings::Code::UnknownLanguage, &name,
                    format!("'{name}' is not among the languages in use, so nothing was counted for it.")));
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
    for name in find_unknown_language_names(&languages, &config.excluded_languages) {
        reported.push(Warning::new(warnings::Code::UnknownExcludedLanguage, &name,
                format!("'{name}' is not among the languages in use, so excluding it changed nothing.")));
    }

    // The ones the narrowing removes are set aside rather than dropped, because a section inside a
    // counted file may still be written in one of them
    let (languages, set_aside) = languages.into_iter().partition(|language|
            (config.languages_of_interest.is_empty()
                    || config.languages_of_interest.iter().any(|x| selects(x, language)))
            && !config.excluded_languages.iter().any(|x| selects(x, language)));

    (languages, set_aside, reported)
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
        assert_eq!(vec!["C#", "Java", "Rust"], names_of(retain_languages_of_interest(languages(), &HashMap::new(), &config).0));

        // asked for by a name that differs in case, which is still the same language
        config.languages_of_interest = vec!["java".to_owned(), "RUST".to_owned()];
        assert_eq!(vec!["Java", "Rust"], names_of(retain_languages_of_interest(languages(), &HashMap::new(), &config).0));

        // and the exclusion applies on top of the selection
        config.excluded_languages = vec!["rust".to_owned()];
        assert_eq!(vec!["Java"], names_of(retain_languages_of_interest(languages(), &HashMap::new(), &config).0));

        // an excluded name on its own leaves everything else
        config.languages_of_interest = Vec::new();
        assert_eq!(vec!["C#", "Java"], names_of(retain_languages_of_interest(languages(), &HashMap::new(), &config).0));

        assert_eq!(vec!["Erlang"], find_unknown_language_names(&languages(), &["java".to_owned(), "Erlang".to_owned()]));
        assert!(find_unknown_language_names(&languages(), &["C#".to_owned()]).is_empty());
    }

    // A language is asked for by the name it carries or by an extension it claims, since the two
    // are what somebody has in front of them: the report shows the name and the files show the
    // extension, and which of the two is the shorter word is an accident of the language.
    #[test]
    fn a_language_is_selected_by_its_name_or_by_an_extension_it_claims() {
        let languages = || languages_claiming(&[("Java", &["java"]), ("C#", &["cs"]), ("Rust", &["rs"])])
                .into_values().collect::<Vec<_>>();
        let extensions = || build_language_map_by(IdentifiedBy::Extension, &keyed_by_name(languages()),
                &HashMap::new(), &HashMap::new()).0;
        let names_of = |languages: Vec<Language>| {
            let mut names = languages.into_iter().map(|x| x.name).collect::<Vec<_>>();
            names.sort();
            names
        };
        let kept = |config: &EngineConfig| names_of(
                retain_languages_of_interest(languages(), &extensions(), config).0);

        let selecting = |names: &[&str]| EngineConfig {
                languages_of_interest: names.iter().map(|x| (*x).to_owned()).collect(), ..Default::default() };
        assert_eq!(vec!["C#"], kept(&selecting(&["cs"])), "an extension did not select its language");
        // and the case of the extension is as free as the case of a name
        assert_eq!(vec!["Rust"], kept(&selecting(&["RS"])));
        // excluding takes the same road
        let excluding = EngineConfig {
                excluded_languages: vec!["java".to_owned(), "rs".to_owned()], ..Default::default() };
        assert_eq!(vec!["C#"], kept(&excluding));

        // an extension that exists is not an unknown name, or the run would warn about a spelling
        // that had just worked
        assert!(find_unknown_language_names(&languages(), &["cs".to_owned(), "RS".to_owned()]).is_empty());
        assert_eq!(vec!["nosuch"], find_unknown_language_names(&languages(), &["nosuch".to_owned()]));
    }

    // Two languages claiming one extension have already been settled for the counting, and asking
    // for that extension has to mean the same language it means everywhere else, or the same word
    // would select one language and count another.
    #[test]
    fn an_extension_two_languages_claim_selects_the_one_that_won_it() {
        let languages = || languages_claiming(&[("Objective-C", &["m", "mm"]), ("MATLAB", &["m"])])
                .into_values().collect::<Vec<_>>();
        let config = EngineConfig { languages_of_interest: vec!["m".to_owned()], ..Default::default() };
        let kept = |priority, forced| {
            let extensions = build_language_map_by(IdentifiedBy::Extension, &keyed_by_name(languages()),
                    priority, forced).0;
            retain_languages_of_interest(languages(), &extensions, &config).0
                    .into_iter().map(|x| x.name).collect::<Vec<_>>()
        };

        let (nothing_decided, no_forcing) = (HashMap::new(), HashMap::new());
        let priority = hashmap!("m".to_owned() => vec!["Objective-C".to_owned(), "MATLAB".to_owned()]);
        let forced = hashmap!("m".to_owned() => "MATLAB".to_owned());

        // the alphabetical tiebreak, which is what the run counts with when nobody has decided
        assert_eq!(vec!["MATLAB"], kept(&nothing_decided, &no_forcing));
        // the priority file's answer
        assert_eq!(vec!["Objective-C"], kept(&priority, &no_forcing));
        // and '--force-language', which beats the priority file here as it does everywhere
        assert_eq!(vec!["MATLAB"], kept(&priority, &forced));
    }

    // A misspelt default is silent otherwise: nothing refuses the language file, since a file being
    // read knows only itself, and every unnamed section goes back to being read with the container's
    // own symbols, which is a comment count that looks perfectly ordinary and is wrong.
    #[test]
    fn a_region_default_that_names_no_language_is_reported() {
        let shell = |default: &str| Language::new("Weblike", ["wbl"], [""; 0], [""; 0], &[("<!--", "-->")], [])
                .with_nested_languages(&[crate::NestedLanguage::of("<script", "</script>", default)]);
        let resolved = |default: &str| Languages::resolve(&EngineConfig::default(),
                vec![shell(default), Language::new("JavaScript", ["js"], ["\""], ["//"], &[], [])],
                &PriorityRules::default()).1
                .into_iter().filter(|x| x.code == warnings::Code::UnknownSectionLanguage).collect::<Vec<_>>();

        assert!(resolved("js").is_empty(), "an extension a language claims was called unknown");
        assert!(resolved("javascript").is_empty(), "a language's own name was called unknown");

        let complained = resolved("javascrpt");
        assert_eq!(1, complained.len(), "a default nothing answers to was not reported");
        assert_eq!("javascrpt", complained[0].subject);
        assert!(complained[0].message.contains("Weblike"), "the message does not name the language that declared it");
    }

    // Returned and not printed, because the command line puts its own colored version on the
    // screen with a suggested spelling next to it.
    #[test]
    fn a_language_that_does_not_exist_reaches_the_document_as_a_warning() {
        let config = EngineConfig {
            languages_of_interest: vec!["Java".to_owned(), "Nolang-Q9".to_owned()],
            ..Default::default()
        };
        let (_, _, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"])]).into_values().collect(), &HashMap::new(), &config);

        let mine = reported.into_iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::Code::UnknownLanguage, mine.code);
        // the counts are sound for what does exist, it is the setting that was not honoured
        assert_eq!("settings", mine.affects().name());
    }

    // The names travel with the bytes because the command line writes each file to disk under the
    // name it came with, prefixed by its folder. Returning the whole embedded path instead passed
    // every test there was, and would have installed 'languages/data/languages/Rust.txt'.
    #[test]
    fn the_shipped_files_carry_bare_names_and_match_the_directory() {
        let raw = get_shipped_language_files_raw();
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
        let cafe = || vec![Language::new("CAFÉ", ["cf"], ["\""], ["//"], &[], []),
                Language::new("Rust", ["rs"], ["\""], ["//"], &[], [])];
        let names_of = |languages: Vec<Language>| languages.into_iter().map(|x| x.name).collect::<Vec<_>>();

        let config = EngineConfig { excluded_languages: vec!["café".to_owned()], ..Default::default() };
        let (kept, _, reported) = retain_languages_of_interest(cafe(), &HashMap::new(), &config);

        assert_eq!(vec!["Rust"], names_of(kept), "the accented name survived an exclusion that names it");
        assert!(reported.is_empty(), "the language was excluded and the run said it does not exist: {reported:?}");

        // and the selection folds case the same way, so asking for it by the other spelling finds it
        let config = EngineConfig { languages_of_interest: vec!["café".to_owned()], ..Default::default() };
        let (kept, _, reported) = retain_languages_of_interest(cafe(), &HashMap::new(), &config);
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
        let (kept, _, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])]).into_values().collect(),
                &HashMap::new(), &config);

        assert_eq!(vec!["Java"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert!(!reported.iter().any(|x| x.subject == "Rust"),
                "'Rust' exists and was reported as missing: {reported:?}");
    }

    // Two definitions of one name: the map keeps the last and the order is the directory's, so
    // renaming a file changes which comment symbols a language counts with. Measured before this:
    // the same source came back as comment=0 code=3 or comment=4 code=1 depending on the file names.
    #[test]
    fn two_definitions_of_one_name_are_reported_against_the_counts() {
        let twice = vec![Language::new("Same", ["aa"], ["\""], ["//"], &[], []),
                Language::new("Same", ["bb"], ["\""], [""; 0], &[("/*", "*/")], []),
                Language::new("Rust", ["rs"], ["\""], ["//"], &[], [])];

        let config = EngineConfig::default();
        let (languages, _) = Languages::resolve(&config, twice, &PriorityRules::default());
        let reported = Languages::resolve(&config,
                vec![Language::new("Same", ["aa"], ["\""], ["//"], &[], []),
                     Language::new("Same", ["bb"], ["\""], [""; 0], &[("/*", "*/")], [])],
                &PriorityRules::default()).1;

        let mine = reported.iter().find(|x| x.code == warnings::Code::DuplicateLanguage)
                .expect("a language declared twice was dropped in silence");
        assert_eq!("Same", mine.subject);
        assert_eq!("counts", mine.affects().name(), "the choice changes numbers, not settings");
        // one of the two really is gone, which is what the warning is about
        assert_eq!(2, languages.into_parts().0.len());
    }

    // A language nobody can name and a language no file can match were both accepted, took a row in
    // every internal map, and contributed nothing. The file parser refuses both, so this is the
    // caller who built one by hand.
    #[test]
    fn a_language_that_cannot_be_named_or_matched_is_dropped_and_reported() {
        let unusable = vec![
            Language::new("   ", ["zz"], ["\""], ["//"], &[], []),
            Language::new("Claims-Nothing", [""; 0], ["\""], ["//"], &[], []),
            Language::new("Rust", ["rs"], ["\""], ["//"], &[], [])];

        let (kept, reported) = drop_the_unusable(unusable);
        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert_eq!(2, reported.len(), "{reported:?}");
        // Two codes and not one, because the consequences differ: dropping the nameless one leaves
        // every '.zz' file counted by nobody, while the one claiming nothing could never have
        // matched a file in the first place
        assert_eq!(Some(warnings::Code::LanguageWithoutName),
                reported.iter().find(|x| x.subject == "zz").map(|x| x.code),
                "the nameless one is named by what it claims");
        assert_eq!(Some(warnings::Code::LanguageClaimsNothing),
                reported.iter().find(|x| x.subject == "Claims-Nothing").map(|x| x.code));
    }

    // A language file may declare filenames and no extension at all, which is what a definition for
    // Makefile or Dockerfile alone looks like. Dropping it took the language out of the run and
    // reported that it claimed nothing, while the name it claimed would have matched files.
    #[test]
    fn a_language_that_claims_only_filenames_is_kept() {
        let by_name_only = Language::new("Docky", [""; 0], ["\""], ["#"], &[], [])
                .with_filenames(&["Dockerfile"]);

        let (kept, reported) = drop_the_unusable(vec![by_name_only]);
        assert_eq!(vec!["Docky"], kept.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), "{reported:?}");
        assert!(reported.is_empty(), "{reported:?}");

        // and it reaches the map that answers for a whole filename
        let (languages, _) = Languages::resolve(&EngineConfig::default(), kept, &PriorityRules::default());
        assert_eq!(Some("Docky"), languages.lookup.of_path(std::path::Path::new("some/dir/Dockerfile")).as_deref());
    }

    // The same pairs are handed to the map of extensions and to the map of whole names, so asking
    // this question inside either of them says it twice, on the screen and in the JSON document.
    #[test]
    fn a_forced_language_that_is_not_available_is_reported_once_and_changes_nothing() {
        let config = EngineConfig {
            forced_languages: hashmap!("py".to_owned() => "cobol".to_owned()),
            ..Default::default()
        };
        let languages = vec![Language::new("Python", ["py"], ["\""], ["#"], &[], [])
                .with_filenames(&["SConstruct"])];

        let (_, reported) = Languages::resolve(&config, languages, &PriorityRules::default());
        let mine = reported.iter().filter(|x| x.code == warnings::Code::UnknownForcedLanguage)
                .collect::<Vec<_>>();
        assert_eq!(1, mine.len(), "said once for each map it could not be used by: {reported:?}");
        // a mapping that did not apply leaves the counts alone, it is the settings that were not honoured
        assert_eq!("settings", mine[0].affects().name());
        assert_eq!("py", mine[0].subject);
        // Names what was asked for and what happened, and nothing a command line can do about it:
        // that sentence belongs to whoever has a command line.
        assert!(mine[0].message.contains("'cobol'"), "{}", mine[0].message);
        assert!(!mine[0].message.contains("--force-language"), "{}", mine[0].message);
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
        let (kept, _, reported) = retain_languages_of_interest(
                languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])]).into_values().collect(),
                &HashMap::new(), &config);

        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        let mine = reported.iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::Code::UnknownExcludedLanguage, mine.code);
        assert_eq!("settings", mine.affects().name());
        // and the one that does exist is excluded without a word about it
        assert!(!reported.iter().any(|x| x.subject == "Java"));
    }
}
