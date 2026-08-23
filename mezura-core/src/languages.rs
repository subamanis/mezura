// Which languages a run has in play, and which of them owns an extension two of them claim. The
// format a language file is written in is 'language_file' next door.
use std::{collections::HashMap, sync::Arc};

use crate::{Language, warnings};
use crate::engine::config::EngineConfig;
use crate::engine::identity::{IdentifiedBy, LanguageLookup, ScopedLookups, build_language_map_by,
        extension_key};
use crate::language_file::ConflictRules;
use crate::warnings::Warning;

// Built by the caller and handed to 'run', rather than inside it, so that its complaints about the
// settings land with the other complaints about settings and not in the middle of a report.
pub struct Languages {
    by_name: HashMap<String, Language>,
    lookups: ScopedLookups,
    nested: NestedLanguageDefinitions,
    // Which settings produced this set, so 'run' can refuse one that would have produced another.
    resolved_against: LanguageSelection
}

impl Languages {
    // The languages baked into this crate, so nothing on the machine is read. The command line reads
    // its own folder instead, because a language file there is the user's to edit.
    pub fn shipped(config: &EngineConfig) -> (Self, Vec<Warning>) {
        Self::resolve(config, parse_shipped_languages(), &parse_shipped_conflict_rules())
    }

    // The shipped set plus languages of the caller's own. Here rather than left to the caller,
    // because doing it by hand means remembering the shipped conflict rules as well: passed a
    // default set instead, a contested extension is settled a different way and the counts come
    // back looking perfectly normal.
    pub fn shipped_with(config: &EngineConfig, extra: impl IntoIterator<Item = Language>)
    -> (Self, Vec<Warning>)
    {
        let mut languages = parse_shipped_languages();
        languages.extend(extra);
        Self::resolve(config, languages, &parse_shipped_conflict_rules())
    }

    // For a caller with languages and conflict rules of its own. Keyed here by each language's own
    // name rather than taken as a map somebody else keyed: in a map whose key and value disagree the
    // key wins, and a language would be counted under a name it does not carry.
    pub fn resolve(config: &EngineConfig, languages: impl IntoIterator<Item = Language>,
            conflicts: &ConflictRules) -> (Self, Vec<Warning>)
    {
        // Unusable ones go first, so that a name nobody can ask for is not in the list when the
        // narrowing below asks whether a name exists. Duplicates are reported last, after the
        // narrowing, so a run that never asked for the language is not told about it.
        let (languages, mut reported) = drop_the_unusable(languages.into_iter().collect());
        reported.extend(find_unknown_module_scopes(config));
        reported.extend(find_unknown_names_of_the_selection(&languages, config));
        reported.extend(find_duplicate_names(&languages));

        let everything = keyed_by_name(languages.clone());
        let mut all_extensions = HashMap::new();
        let mut in_play : HashMap<String, Language> = HashMap::new();
        let mut whole_run = LanguageLookup::default();
        let mut per_module = HashMap::new();
        // The run's own answer first, so that what it resolves an extension to is the map the
        // sections inside container files are read against.
        for module in std::iter::once(None).chain(find_modules_with_rules_of_their_own(config).into_iter().map(Some)) {
            let scope = resolve_one_scope(&languages, &everything, config, module.as_deref(), conflicts);
            reported.extend(scope.reported);
            in_play.extend(scope.in_play);
            match module {
                None => (whole_run, all_extensions) = (scope.lookup, scope.all_extensions),
                Some(module) => { per_module.insert(module, scope.lookup); }
            }
        }
        reported.extend(find_unknown_forced_languages(&in_play, &config.forced_languages));
        // Two scopes resolving the same contest the same way say so once. Without this a run that
        // scopes anything repeats every tiebreak of the whole language set once per module.
        let mut already_said = std::collections::HashSet::new();
        reported.retain(|warning| already_said.insert((warning.code.name(), warning.subject.clone(),
                warning.message.clone())));

        // A section names its language whatever the run narrowed itself to, which is why the
        // languages the narrowing set aside are kept and the map handed over is the one built over
        // everything. Only carried when a language in play declares regions, so an ordinary run
        // holds no second copy of anything.
        let mut nested = NestedLanguageDefinitions::default();
        let set_aside = everything.into_iter().filter(|(name, _)| !in_play.contains_key(name))
                .collect::<HashMap<_,_>>();
        if in_play.values().chain(set_aside.values()).any(|language| !language.nested_languages.is_empty()) {
            reported.extend(find_unresolvable_region_defaults(&in_play, &set_aside, &all_extensions));
            nested = NestedLanguageDefinitions { set_aside, extension_to_name: all_extensions };
        }

        (Languages { by_name: in_play, lookups: ScopedLookups::of(whole_run, per_module),
                nested, resolved_against: LanguageSelection::of(config) }, reported)
    }

    pub(crate) fn describe_the_same_selection_as(&self, config: &EngineConfig) -> bool {
        self.resolved_against == LanguageSelection::of(config)
    }

    pub(crate) fn into_parts(self) -> (HashMap<String, Language>, ScopedLookups, NestedLanguageDefinitions) {
        (self.by_name, self.lookups, self.nested)
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

// The rules this crate ships for settling an extension or a filename that two languages both claim.
pub fn parse_shipped_conflict_rules() -> ConflictRules {
    crate::language_file::parse_conflict_rules(&String::from_utf8_lossy(get_shipped_conflict_rules_raw())).0
}

// The bytes as they were authored, comments and layout included, so what the installer puts in the
// user's folder is a file made to be read and edited. Public because that installer is a separate
// crate and cannot reach into this one's 'data/'; plain tuples and not the embedder's own file type,
// so a release of 'include_dir' is never a breaking change of ours.
pub fn get_shipped_language_files_raw() -> Vec<(&'static str, &'static [u8])> {
    include_dir::include_dir!("data/languages").files.iter()
            .map(|file| (std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path),
                    file.contents))
            .collect()
}

pub fn get_shipped_conflict_rules_raw() -> &'static [u8] {
    include_bytes!("../data/language_conflicts.txt")
}

// The names that were asked for and no language answers to, in the order they were given. A
// language answers to the name it carries and to every extension it claims, so 'js' is not an
// unknown name while some language counts '.js' files. Which of two languages owns a contested
// extension is a different question, settled where the narrowing happens.
pub fn find_unknown_language_names(languages: &[Language], wanted: &[String]) -> Vec<String> {
    wanted.iter().filter(|wanted| !languages.iter().any(|language|
                    is_the_same_language_name(&language.name, wanted)
                    || language.extensions.iter().any(|extension| is_the_same_language_name(extension, wanted))))
            .cloned().collect()
}

// Every place that matches a name goes through this one: choosing, excluding, forcing an extension,
// and the conflict rules.
//
// 'to_lowercase' and not 'eq_ignore_ascii_case', which agree until a name has a letter outside ASCII:
// mixing the two takes 'CAFÉ' excluded as 'café' out of the count by one rule while the other
// reports it, in the same run, as a name that does not exist.
pub(crate) fn is_the_same_language_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

// By the name each language carries. A later declaration of a name wins, which is what happens when
// a directory holds two files for one language.
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
    forced: HashMap<String, String>,
    // Empty until a module is given a rule of its own, and only then do the names the targets
    // declare decide anything: each rule reaches its module by name, so a run whose targets name
    // other modules would leave every one of those rules doing nothing, in silence.
    modules: Vec<String>
}

impl LanguageSelection {
    fn of(config: &EngineConfig) -> Self {
        // Only the half after the slash is folded. A module name is matched exactly, the way the
        // targets that declare it are, so 'IOS/m' and 'ios/m' are rules for two different modules.
        let folded = |names: &crate::engine::config::LanguageNames| {
            let mut names = names.to_written_form().iter().map(|written| {
                let (module, name) = crate::engine::config::split_off_module_scope(written);
                crate::engine::config::format_module_scope(module, &name.to_lowercase())
            }).collect::<Vec<_>>();
            names.sort();
            names
        };

        LanguageSelection {
            of_interest: folded(&config.languages_of_interest),
            excluded: folded(&config.excluded_languages),
            forced: config.forced_languages.to_written_form().iter().map(|(written, language)| {
                let (module, claimed) = crate::engine::config::split_off_module_scope(written);
                (crate::engine::config::format_module_scope(module, &extension_key(claimed)),
                        language.to_lowercase())
            }).collect(),
            modules: match config.forced_languages.is_scoped() || config.languages_of_interest.is_scoped()
                    || config.excluded_languages.is_scoped() {
                false => Vec::new(),
                true => {
                    let mut declared = config.targets.iter()
                            .filter_map(|target| target.module.clone()).collect::<Vec<_>>();
                    declared.sort();
                    declared.dedup();
                    declared
                }
            }
        }
    }
}

struct ResolvedScope {
    in_play: HashMap<String, Language>,
    lookup: LanguageLookup,
    // Over every language and not only the ones in play, which is what lets '--languages m' name
    // one and a section inside a container file name another the narrowing took out.
    all_extensions: HashMap<String, Arc<str>>,
    reported: Vec<Warning>
}

// Everything one scope answers for itself: which languages are in play there, what identifies them,
// and which language an extension names when '--languages' is given one.
fn resolve_one_scope(languages: &[Language], everything: &HashMap<String, Language>, config: &EngineConfig,
        module: Option<&str>, conflicts: &ConflictRules) -> ResolvedScope
{
    let forced = config.forced_languages.get_rules_of_module(module);
    // Over everything, before the narrowing, because it answers two questions the narrowed set
    // cannot: which language an extension names when '--languages' is given one, and what a section
    // inside a container file is written in when the run narrowed that language away. Its own
    // complaints about contested extensions are dropped, since the narrowed build below makes them,
    // and a contest between two languages the run then leaves out is not news.
    let (all_extensions, _) = build_language_map_by(IdentifiedBy::Extension, everything,
            &conflicts.by_extension, &forced);
    let by_name = keyed_by_name(retain_languages_of_interest(languages.to_vec(), &all_extensions,
            config.languages_of_interest.get_names_of_module(module),
            config.excluded_languages.get_names_of_module(module)));

    let mut reported = Vec::new();
    let (by_extension, report) = build_language_map_by(IdentifiedBy::Extension, &by_name,
            &conflicts.by_extension, &forced);
    reported.extend(report.collect_warnings());
    // The forced pairs go to both, since '--force-language Makefile=python' and '--force-language
    // txt=python' are the same sentence and the reader has no reason to know which map answers
    let (by_filename, filename_report) = build_language_map_by(IdentifiedBy::Filename, &by_name,
            &conflicts.by_filename, &forced);
    reported.extend(filename_report.collect_warnings());
    // The conflicts file has no block for a contested interpreter yet; the forced pairs reach this
    // map like the other two, which is how such a contest would be settled by hand.
    let (by_shebang, shebang_report) = build_language_map_by(IdentifiedBy::Shebang, &by_name,
            &HashMap::new(), &forced);
    reported.extend(shebang_report.collect_warnings());

    ResolvedScope { in_play: by_name, lookup: LanguageLookup { by_extension, by_filename, by_shebang },
            all_extensions, reported }
}

// Sorted and without repeats, so that the same settings resolve their modules in the same order
// however the three lists were written.
fn find_modules_with_rules_of_their_own(config: &EngineConfig) -> Vec<String> {
    let mut named = config.forced_languages.get_module_names()
            .chain(config.languages_of_interest.get_module_names())
            .chain(config.excluded_languages.get_module_names())
            .map(str::to_owned).collect::<Vec<_>>();
    named.sort();
    named.dedup();
    named
}

// A rule written for a module the run never declared changes nothing, and the counts it was meant to
// change come out looking perfectly ordinary. The names are matched exactly, as the targets match
// them, so a difference in capitalisation lands here too.
fn find_unknown_module_scopes(config: &EngineConfig) -> Vec<Warning> {
    let mut declared = config.targets.iter().filter_map(|target| target.module.as_deref()).collect::<Vec<_>>();
    declared.sort_unstable();
    declared.dedup();

    find_modules_with_rules_of_their_own(config).into_iter()
            .filter(|named| !declared.contains(&named.as_str()))
            .map(|named| Warning::new(warnings::Code::UnknownModuleScope, &named, match declared.is_empty() {
                true => format!("'{named}' is written as the module a rule belongs to, and this run declares no \
modules at all, so the rule was not used."),
                false => format!("'{named}' is written as the module a rule belongs to, and this run declares no \
module of that name. It declares {}, so the rule was not used.",
                        declared.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", "))
            }))
            .collect()
}

// Over every scope at once, because whether a language exists is not a question one module answers
// differently, and naming the same missing language in two of them is one mistake and not two.
fn find_unknown_names_of_the_selection(languages: &[Language], config: &EngineConfig) -> Vec<Warning> {
    let mut reported = Vec::new();
    if !config.languages_of_interest.is_empty() {
        for name in find_unknown_language_names(languages, &config.languages_of_interest.get_all_names()) {
            reported.push(Warning::new(warnings::Code::UnknownLanguage, &name,
                    format!("'{name}' is not among the languages in use, so nothing was counted for it.")));
        }
    }

    // Asked of the whole list and not of what the selection above left of it: checked after the
    // narrowing, '--languages Java --exclude-languages Rust' reports that Rust does not exist.
    //
    // Under a code of its own, and not the one above it, because the command line has already put
    // that one on the screen with a suggested spelling and keeps it only for the document. This one
    // has no other voice.
    for name in find_unknown_language_names(languages, &config.excluded_languages.get_all_names()) {
        reported.push(Warning::new(warnings::Code::UnknownExcludedLanguage, &name,
                format!("'{name}' is not among the languages in use, so excluding it changed nothing.")));
    }

    reported
}

// Asked once, and not inside the map building, which runs once per kind of identity and per scope
// and is handed the same pairs every time: asking it there says the same thing several times over.
fn find_unknown_forced_languages(by_name: &HashMap<String, Language>,
        forced: &crate::engine::config::ForcedLanguages) -> Vec<Warning>
{
    let mut names = by_name.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();

    let written = forced.to_written_form();
    let mut unknown = written.iter()
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
// knows only itself. Asked here against exactly the two maps the section lookup will consult, so the
// check and what it predicts cannot drift. Left unreported it is silent and expensive: every unnamed
// section goes back to being read with the container's own symbols, so a '//' inside a script block
// counts as code.
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
// counts. Against the counts and not the settings, because the two definitions disagree about
// comment symbols and the losing one takes its extensions out of the run with it.
//
// Grouped through 'is_the_same_language_name' and not by exact spelling, since 'Rust' and 'rust' are
// one language to '--languages', '--exclude-languages', '--force-language' and the conflicts file,
// all of which fold case.
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
        // Identical spellings collapse into one entry of the map and one of them is simply gone;
        // spellings that differ in case are separate entries that both survive, take a row each in
        // the report and split the count of one language between them.
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

// What one scope leaves in play. The ones it removes are not dropped by the caller either, because
// a section inside a counted file may still be written in one of them.
fn retain_languages_of_interest(languages: Vec<Language>, extensions: &HashMap<String, Arc<str>>,
        of_interest: &[String], excluded: &[String]) -> Vec<Language>
{
    // A spelling selects a language by the name it carries, or by an extension it owns. The
    // ownership is read from the map the counting itself uses, so '--languages m' means the same
    // language that every '.m' file is counted as here, whether that was settled by the conflicts
    // file, by '--force-language' or by the tiebreak.
    let selects = |spelling: &String, language: &Language| {
        is_the_same_language_name(&language.name, spelling)
                || extensions.get(&extension_key(spelling))
                        .is_some_and(|owner| owner.as_ref() == language.name)
    };

    languages.into_iter().filter(|language|
            (of_interest.is_empty() || of_interest.iter().any(|x| selects(x, language)))
            && !excluded.iter().any(|x| selects(x, language)))
            .collect()
}

#[cfg(test)]
mod language_selection_tests {
    use super::*;
    use crate::StringRules;
    use crate::languages_claiming;

    #[test]
    fn the_run_narrows_the_languages_and_records_a_name_that_does_not_exist() {
        let languages = || languages_claiming(&[("Java", &["java"]), ("C#", &["cs"]), ("Rust", &["rs"])])
                .into_values().collect::<Vec<_>>();
        let names_of = |languages: Vec<Language>| {
            let mut names = languages.into_iter().map(|x| x.name).collect::<Vec<_>>();
            names.sort();
            names
        };

        let kept = |of_interest: &[&str], excluded: &[&str]| names_of(
                retain_languages_of_interest(languages(), &HashMap::new(), &owned(of_interest), &owned(excluded)));

        assert_eq!(vec!["C#", "Java", "Rust"], kept(&[], &[]));
        assert_eq!(vec!["Java", "Rust"], kept(&["java", "RUST"], &[]));
        // the exclusion applies on top of the selection
        assert_eq!(vec!["Java"], kept(&["java", "RUST"], &["rust"]));
        // and on its own it leaves everything else
        assert_eq!(vec!["C#", "Java"], kept(&[], &["rust"]));

        assert_eq!(vec!["Erlang"], find_unknown_language_names(&languages(), &["java".to_owned(), "Erlang".to_owned()]));
        assert!(find_unknown_language_names(&languages(), &["C#".to_owned()]).is_empty());
    }

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
        let kept = |of_interest: &[&str], excluded: &[&str]| names_of(
                retain_languages_of_interest(languages(), &extensions(), &owned(of_interest), &owned(excluded)));

        assert_eq!(vec!["C#"], kept(&["cs"], &[]), "an extension did not select its language");
        assert_eq!(vec!["Rust"], kept(&["RS"], &[]));
        assert_eq!(vec!["C#"], kept(&[], &["java", "rs"]));

        // an extension that exists is not an unknown name, or the run would warn about a spelling
        // that had just worked
        assert!(find_unknown_language_names(&languages(), &["cs".to_owned(), "RS".to_owned()]).is_empty());
        assert_eq!(vec!["nosuch"], find_unknown_language_names(&languages(), &["nosuch".to_owned()]));
    }

    #[test]
    fn an_extension_two_languages_claim_selects_the_one_that_won_it() {
        let languages = || languages_claiming(&[("Objective-C", &["m", "mm"]), ("MATLAB", &["m"])])
                .into_values().collect::<Vec<_>>();
        let kept = |conflicts, forced| {
            let extensions = build_language_map_by(IdentifiedBy::Extension, &keyed_by_name(languages()),
                    conflicts, forced).0;
            retain_languages_of_interest(languages(), &extensions, &["m".to_owned()], &[])
                    .into_iter().map(|x| x.name).collect::<Vec<_>>()
        };

        let (nothing_decided, no_forcing) = (HashMap::new(), HashMap::new());
        let conflicts = hashmap!("m".to_owned() => vec!["Objective-C".to_owned(), "MATLAB".to_owned()]);
        let forced = hashmap!("m".to_owned() => "MATLAB".to_owned());

        assert_eq!(vec!["MATLAB"], kept(&nothing_decided, &no_forcing));
        assert_eq!(vec!["Objective-C"], kept(&conflicts, &no_forcing));
        // forcing beats the conflicts file here as it does everywhere
        assert_eq!(vec!["MATLAB"], kept(&conflicts, &forced));
    }

    #[test]
    fn a_region_default_that_names_no_language_is_reported() {
        let shell = |default: &str| Language::new("Weblike", ["wbl"], StringRules::escaping_nothing(),
                        [""; 0], &[("<!--", "-->")], [])
                .with_nested_languages(&[crate::NestedLanguage::of("<script", "</script>", default)]);
        let resolved = |default: &str| Languages::resolve(&EngineConfig::default(),
                vec![shell(default), Language::new("JavaScript", ["js"], StringRules::escaping_nothing(), ["//"], &[], [])],
                &ConflictRules::default()).1
                .into_iter().filter(|x| x.code == warnings::Code::UnknownSectionLanguage).collect::<Vec<_>>();

        assert!(resolved("js").is_empty(), "an extension a language claims was called unknown");
        assert!(resolved("javascript").is_empty(), "a language's own name was called unknown");

        let complained = resolved("javascrpt");
        assert_eq!(1, complained.len(), "a default nothing answers to was not reported");
        assert_eq!("javascrpt", complained[0].subject);
        assert!(complained[0].message.contains("Weblike"), "the message does not name the language that declared it");
    }

    #[test]
    fn a_language_that_does_not_exist_reaches_the_document_as_a_warning() {
        let config = EngineConfig {
            languages_of_interest: owned(&["Java", "Nolang-Q9"]).into(),
            ..Default::default()
        };
        let reported = find_unknown_names_of_the_selection(
                &languages_claiming(&[("Java", &["java"])]).into_values().collect::<Vec<_>>(), &config);

        let mine = reported.into_iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::Code::UnknownLanguage, mine.code);
        assert_eq!("settings", mine.affects().name());
    }

    // The command line writes each file to disk under the name it came with, prefixed by its
    // folder, so an embedded path here installs 'languages/data/languages/Rust.txt'.
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

    #[test]
    fn a_language_name_outside_ascii_is_excluded_and_not_reported_as_missing() {
        let cafe = || vec![Language::new("CAFÉ", ["cf"], StringRules::escaping_nothing(), ["//"], &[], []),
                Language::new("Rust", ["rs"], StringRules::escaping_nothing(), ["//"], &[], [])];
        let names_of = |languages: Vec<Language>| languages.into_iter().map(|x| x.name).collect::<Vec<_>>();

        let config = EngineConfig { excluded_languages: owned(&["café"]).into(), ..Default::default() };
        let kept = retain_languages_of_interest(cafe(), &HashMap::new(), &[], &owned(&["café"]));
        let reported = find_unknown_names_of_the_selection(&cafe(), &config);

        assert_eq!(vec!["Rust"], names_of(kept), "the accented name survived an exclusion that names it");
        assert!(reported.is_empty(), "the language was excluded and the run said it does not exist: {reported:?}");

        let config = EngineConfig { languages_of_interest: owned(&["café"]).into(), ..Default::default() };
        let kept = retain_languages_of_interest(cafe(), &HashMap::new(), &owned(&["café"]), &[]);
        let reported = find_unknown_names_of_the_selection(&cafe(), &config);
        assert_eq!(vec!["CAFÉ"], names_of(kept));
        assert!(reported.is_empty(), "{reported:?}");
    }

    #[test]
    fn excluding_a_language_outside_the_selection_is_not_reported_as_missing() {
        let config = EngineConfig {
            languages_of_interest: owned(&["Java"]).into(),
            excluded_languages: owned(&["Rust"]).into(),
            ..Default::default()
        };
        let available = languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])])
                .into_values().collect::<Vec<_>>();
        let kept = retain_languages_of_interest(available.clone(), &HashMap::new(),
                &owned(&["Java"]), &owned(&["Rust"]));
        let reported = find_unknown_names_of_the_selection(&available, &config);

        assert_eq!(vec!["Java"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert!(!reported.iter().any(|x| x.subject == "Rust"),
                "'Rust' exists and was reported as missing: {reported:?}");
    }

    // With the same source and only the file names changed, the two definitions gave comment=0
    // code=3 and comment=4 code=1.
    #[test]
    fn two_definitions_of_one_name_are_reported_against_the_counts() {
        let twice = vec![Language::new("Same", ["aa"], StringRules::escaping_nothing(), ["//"], &[], []),
                Language::new("Same", ["bb"], StringRules::escaping_nothing(), [""; 0], &[("/*", "*/")], []),
                Language::new("Rust", ["rs"], StringRules::escaping_nothing(), ["//"], &[], [])];

        let config = EngineConfig::default();
        let (languages, _) = Languages::resolve(&config, twice, &ConflictRules::default());
        let reported = Languages::resolve(&config,
                vec![Language::new("Same", ["aa"], StringRules::escaping_nothing(), ["//"], &[], []),
                     Language::new("Same", ["bb"], StringRules::escaping_nothing(), [""; 0], &[("/*", "*/")], [])],
                &ConflictRules::default()).1;

        let mine = reported.iter().find(|x| x.code == warnings::Code::DuplicateLanguage)
                .expect("a language declared twice was dropped in silence");
        assert_eq!("Same", mine.subject);
        assert_eq!("counts", mine.affects().name(), "the choice changes numbers, not settings");
        // one of the two really is gone
        assert_eq!(2, languages.into_parts().0.len());
    }

    #[test]
    fn a_language_that_cannot_be_named_or_matched_is_dropped_and_reported() {
        let unusable = vec![
            Language::new("   ", ["zz"], StringRules::escaping_nothing(), ["//"], &[], []),
            Language::new("Claims-Nothing", [""; 0], StringRules::escaping_nothing(), ["//"], &[], []),
            Language::new("Rust", ["rs"], StringRules::escaping_nothing(), ["//"], &[], [])];

        let (kept, reported) = drop_the_unusable(unusable);
        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        assert_eq!(2, reported.len(), "{reported:?}");
        assert_eq!(Some(warnings::Code::LanguageWithoutName),
                reported.iter().find(|x| x.subject == "zz").map(|x| x.code),
                "the nameless one is named by what it claims");
        assert_eq!(Some(warnings::Code::LanguageClaimsNothing),
                reported.iter().find(|x| x.subject == "Claims-Nothing").map(|x| x.code));
    }

    // What a definition for Makefile or Dockerfile alone looks like: filenames and no extension.
    #[test]
    fn a_language_that_claims_only_filenames_is_kept() {
        let by_name_only = Language::new("Docky", [""; 0], StringRules::escaping_nothing(), ["#"], &[], [])
                .with_filenames(&["Dockerfile"]);

        let (kept, reported) = drop_the_unusable(vec![by_name_only]);
        assert_eq!(vec!["Docky"], kept.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), "{reported:?}");
        assert!(reported.is_empty(), "{reported:?}");

        let (languages, _) = Languages::resolve(&EngineConfig::default(), kept, &ConflictRules::default());
        assert_eq!(Some("Docky"), languages.lookups.get_of_module_named(None)
                .of_path(std::path::Path::new("some/dir/Dockerfile")).as_deref());
    }

    #[test]
    fn a_forced_language_that_is_not_available_is_reported_once_and_changes_nothing() {
        let config = EngineConfig {
            forced_languages: hashmap!("py".to_owned() => "cobol".to_owned()).into(),
            ..Default::default()
        };
        let languages = vec![Language::new("Python", ["py"], StringRules::escaping_nothing(), ["#"], &[], [])
                .with_filenames(&["SConstruct"])];

        let (_, reported) = Languages::resolve(&config, languages, &ConflictRules::default());
        let mine = reported.iter().filter(|x| x.code == warnings::Code::UnknownForcedLanguage)
                .collect::<Vec<_>>();
        assert_eq!(1, mine.len(), "said once for each map it could not be used by: {reported:?}");
        assert_eq!("settings", mine[0].affects().name());
        assert_eq!("py", mine[0].subject);
        // The message names what was asked for and leaves '--force-language' to whoever has a
        // command line
        assert!(mine[0].message.contains("'cobol'"), "{}", mine[0].message);
        assert!(!mine[0].message.contains("--force-language"), "{}", mine[0].message);
    }

    #[test]
    fn excluding_a_language_that_does_not_exist_is_reported_too() {
        let config = EngineConfig {
            excluded_languages: owned(&["Java", "Nolang-Q9"]).into(),
            ..Default::default()
        };
        let available = languages_claiming(&[("Java", &["java"]), ("Rust", &["rs"])])
                .into_values().collect::<Vec<_>>();
        let kept = retain_languages_of_interest(available.clone(), &HashMap::new(), &[],
                &owned(&["Java", "Nolang-Q9"]));
        let reported = find_unknown_names_of_the_selection(&available, &config);

        assert_eq!(vec!["Rust"], kept.into_iter().map(|x| x.name).collect::<Vec<_>>());
        let mine = reported.iter().find(|x| x.subject == "Nolang-Q9").unwrap();
        assert_eq!(warnings::Code::UnknownExcludedLanguage, mine.code);
        assert_eq!("settings", mine.affects().name());
        assert!(!reported.iter().any(|x| x.subject == "Java"));
    }

    // The shipped conflict rules are the half a caller doing this by hand forgets, and forgetting
    // them is silent: 'm' is claimed by both Objective-C and MATLAB, and without the rules the
    // contest is settled alphabetically instead, so a MATLAB file is counted as Objective-C.
    #[test]
    fn adding_a_language_of_your_own_keeps_the_shipped_ones_and_their_conflict_rules() {
        let config = EngineConfig::new(["./"]);
        let mine = Language::new("Nolang-Q9", ["nolangq9"], StringRules::escaping_nothing(), ["//"], &[], []);
        let (by_name, lookup, _) = Languages::shipped_with(&config, [mine]).0.into_parts();
        let (shipped_by_name, shipped_lookup, _) = Languages::shipped(&config).0.into_parts();

        assert!(by_name.contains_key("Nolang-Q9"), "the language of my own was dropped");
        assert_eq!(shipped_by_name.len() + 1, by_name.len(), "the shipped ones went with it");
        assert_eq!(shipped_lookup.get_of_module_named(None).of_path(std::path::Path::new("a.m")),
                lookup.get_of_module_named(None).of_path(std::path::Path::new("a.m")),
                "a contested extension was settled differently, so the conflict rules were lost");
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|x| (*x).to_owned()).collect()
    }
}
