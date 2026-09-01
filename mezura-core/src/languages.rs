//! Which languages a run has in play, and which of them owns an extension two of them claim. The
//! format a language file is written in is [`crate::language_file`] next door.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{Language, warnings};
use crate::engine::config::{EngineConfig, ForcedLanguages, LanguageNames, format_module_scope,
        split_off_module_scope};
use crate::engine::identity::{IdentifiedBy, IdentityReport, LanguageLookup, ResolvedBy,
        ScopedLookups, build_language_map_by, extension_key, find_language_named};
use crate::language_file::ConflictRules;
use crate::warnings::Warning;

/// The languages one run counts with, narrowed by its settings and with every contested extension
/// already settled.
///
/// Whichever settings it was built against are the only ones it may be counted with.
pub struct Languages {
    by_name: HashMap<String, Language>,
    lookups: ScopedLookups,
    nested: NestedLanguageDefinitions,
    // Which settings produced this set, so 'run' can refuse one that would have produced another.
    resolved_against: LanguageSelection
}

impl Languages {
    /// The languages baked into this crate, so nothing on the machine is read.
    ///
    /// The warnings beside them are what the settings got wrong: a language name nothing answers
    /// to, a rule written for a module no target declares, an extension two languages claim.
    pub fn shipped(config: &EngineConfig) -> (Self, Vec<Warning>) {
        Self::resolve(config, parse_shipped_languages(), &parse_shipped_conflict_rules())
    }

    /// The shipped set plus language definitions the caller wrote itself, settled by the shipped
    /// conflict rules.
    pub fn shipped_with(config: &EngineConfig, extra: impl IntoIterator<Item = Language>)
    -> (Self, Vec<Warning>)
    {
        let mut languages = parse_shipped_languages();
        languages.extend(extra);
        Self::resolve(config, languages, &parse_shipped_conflict_rules())
    }

    /// For a caller with languages and conflict rules of its own, such as one reading a directory
    /// of language files the user may edit.
    pub fn resolve(config: &EngineConfig, languages: impl IntoIterator<Item = Language>,
            conflicts: &ConflictRules) -> (Self, Vec<Warning>)
    {
        // Unusable ones go first, so that a name nobody can ask for is not in the list when the
        // narrowing below asks whether a name exists. Duplicates are reported last, after the
        // narrowing, so a run that never asked for the language is not told about it.
        let (mut languages, mut reported) = drop_the_unusable(languages.into_iter().collect());
        reported.extend(drop_the_pairs_that_never_fire(&mut languages));
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
        // Without this a run that scopes anything repeats every tiebreak once per module.
        let mut already_said = HashSet::new();
        reported.retain(|warning| already_said.insert((warning.code.name(), warning.subject.clone(),
                warning.message.clone())));

        // A section can be written in a language the narrowing took out, so those definitions are
        // kept. Only when something in play declares regions, or an ordinary run pays for a copy.
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

// Written out: a derived one prints every symbol of every language in play, several hundred lines
// of it for the shipped set.
impl std::fmt::Debug for Languages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut in_play = self.by_name.keys().map(String::as_str).collect::<Vec<_>>();
        in_play.sort_unstable();

        f.debug_struct("Languages").field("in_play", &in_play)
                .field("kept_for_nested_sections", &self.nested.set_aside.len()).finish_non_exhaustive()
    }
}

// Empty on any run where no language declares regions.
#[derive(Default)]
pub(crate) struct NestedLanguageDefinitions {
    pub set_aside: HashMap<String, Language>,
    pub extension_to_name: HashMap<String, std::sync::Arc<str>>,
}

// What this crate ships, parsed for counting and raw for installing.

/// The shipped language definitions, parsed.
pub fn parse_shipped_languages() -> Vec<Language> {
    // 'every_shipped_language_file_parses' is what guarantees these all parse. One that did not
    // would be left out rather than panic here.
    get_shipped_language_files_raw().into_iter()
            .filter_map(|(_, contents)| crate::language_file::parse_language(&String::from_utf8_lossy(contents)))
            .collect()
}

/// The shipped rules for settling an extension or a file name that two languages both claim.
pub fn parse_shipped_conflict_rules() -> ConflictRules {
    crate::language_file::parse_conflict_rules(&String::from_utf8_lossy(get_shipped_conflict_rules_raw())).0
}

/// The shipped language files as they were authored, file name and bytes, comments and layout
/// included, for a caller writing them out into a folder somebody is meant to read and edit.
// Plain tuples and not the embedder's own file type, so a release of 'include_dir' is never a
// breaking change of ours.
pub fn get_shipped_language_files_raw() -> Vec<(&'static str, &'static [u8])> {
    include_dir::include_dir!("data/languages").files.iter()
            .map(|file| (std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path),
                    file.contents))
            .collect()
}

/// The same for the shipped conflict rules, whose file name is [`crate::LANGUAGE_CONFLICTS_FILE_NAME`].
pub fn get_shipped_conflict_rules_raw() -> &'static [u8] {
    include_bytes!("../data/language_conflicts.txt")
}

/// The names that were asked for and no language answers to, in the order they were given.
///
/// A language answers to the name it carries and to every extension it claims, so `js` is not an
/// unknown name while some language counts `.js` files.
pub fn find_unknown_language_names(languages: &[Language], wanted: &[String]) -> Vec<String> {
    wanted.iter().filter(|wanted| !languages.iter().any(|language|
                    is_the_same_language_name(&language.name, wanted)
                    || language.extensions.iter().any(|extension| is_the_same_language_name(extension, wanted))))
            .cloned().collect()
}

// 'to_lowercase' and not 'eq_ignore_ascii_case', which agree until a name has a letter outside ASCII:
// mixing the two takes 'CAFÉ' excluded as 'café' out of the count by one rule while the other
// reports it, in the same run, as a name that does not exist.
pub(crate) fn is_the_same_language_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

// A later declaration of a name wins, which is what a directory holding two files for one language
// comes down to.
pub(crate) fn keyed_by_name(languages: impl IntoIterator<Item = Language>) -> HashMap<String, Language> {
    languages.into_iter().map(|language| (language.name.clone(), language)).collect()
}

// The whole of what building a 'Languages' reads from the settings, normalised the way the matching
// is. Neither order nor case matters, so two settings that would produce this same set compare
// equal and no honest run is refused.
#[derive(PartialEq, Eq, Debug)]
struct LanguageSelection {
    of_interest: Vec<String>,
    excluded: Vec<String>,
    forced: HashMap<String, String>,
    // Empty until a module is given a rule of its own. Only then does a name the targets declare
    // decide anything.
    modules: Vec<String>,
    use_heuristics: bool
}

impl LanguageSelection {
    fn of(config: &EngineConfig) -> Self {
        // Only the half after the slash is folded. A module name is matched exactly, the way the
        // targets that declare it are, so 'IOS/m' and 'ios/m' are rules for two different modules.
        let folded = |names: &LanguageNames| {
            let mut names = names.to_written_form().iter().map(|written| {
                let (module, name) = split_off_module_scope(written);
                format_module_scope(module, &name.to_lowercase())
            }).collect::<Vec<_>>();
            names.sort();
            names
        };

        LanguageSelection {
            of_interest: folded(&config.languages_of_interest),
            excluded: folded(&config.excluded_languages),
            forced: config.forced_languages.to_written_form().iter().map(|(written, language)| {
                let (module, claimed) = split_off_module_scope(written);
                (format_module_scope(module, &extension_key(claimed)),
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
            },
            use_heuristics: config.use_heuristics
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

fn resolve_one_scope(languages: &[Language], everything: &HashMap<String, Language>, config: &EngineConfig,
        module: Option<&str>, conflicts: &ConflictRules) -> ResolvedScope
{
    let forced = config.forced_languages.get_rules_of_module(module);
    // Its complaints about contested extensions are dropped. The narrowed build below makes them,
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

    let extension_rules = if config.use_heuristics {
        build_extension_rules(find_contested_with_evidence(&report, &by_name), conflicts, &forced)
    } else {HashMap::new()};
    ResolvedScope { in_play: by_name,
            lookup: LanguageLookup { by_extension, by_filename, by_shebang, extension_rules },
            all_extensions, reported }
}

// A forced extension is the user's outright answer, so no marker takes its files out of the count.
fn build_extension_rules(contested: HashMap<String, Arc<[Arc<str>]>>, conflicts: &ConflictRules,
        forced: &HashMap<String, String>) -> HashMap<String, Arc<crate::engine::identity::ExtensionRules>>
{
    let empty = Vec::new();
    let forced_keys = forced.keys().map(|x| extension_key(x)).collect::<HashSet<_>>();
    let mut rules: HashMap<String, crate::engine::identity::ExtensionRules> = contested.into_iter()
            .map(|(extension, contenders)| (extension,
                    crate::engine::identity::ExtensionRules { contenders: Some(contenders), not_code: None }))
            .collect();
    for extension in conflicts.not_code_line_starts.keys().chain(conflicts.not_code_line_contains.keys()) {
        if forced_keys.contains(extension) {
            continue;
        }
        let starts = conflicts.not_code_line_starts.get(extension).unwrap_or(&empty);
        let contains = conflicts.not_code_line_contains.get(extension).unwrap_or(&empty);
        if let Some(matcher) = crate::engine::file_parser::IdentificationMatcher::of(starts, contains) {
            rules.entry(extension.clone())
                    .or_insert(crate::engine::identity::ExtensionRules { contenders: None, not_code: None })
                    .not_code = Some(matcher);
        }
    }
    rules.into_iter().map(|(extension, rules)| (extension, Arc::new(rules))).collect()
}

fn find_contested_with_evidence(report: &IdentityReport, by_name: &HashMap<String, Language>)
        -> HashMap<String, Arc<[Arc<str>]>>
{
    report.contested.iter()
            .filter(|contest| contest.resolved_by != ResolvedBy::ForceLang)
            .filter_map(|contest| {
                let claimants = std::iter::once(&contest.winner).chain(&contest.losers)
                        .filter(|name| by_name.contains_key(name.as_str()))
                        .map(|name| Arc::from(name.as_str()))
                        .collect::<Vec<Arc<str>>>();
                let declares_evidence = |name: &Arc<str>| by_name.get(name.as_ref())
                        .is_some_and(Language::declares_identification);
                (claimants.len() > 1 && claimants.iter().any(declares_evidence))
                        .then(|| (contest.identity.clone(), claimants.into()))
            }).collect()
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

// The names are matched exactly, as the targets match them, so a difference in capitalisation lands
// here too.
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

// Over every scope at once. Naming the same missing language in two of them is one mistake, not two.
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
    for name in find_unknown_language_names(languages, &config.excluded_languages.get_all_names()) {
        reported.push(Warning::new(warnings::Code::UnknownExcludedLanguage, &name,
                format!("'{name}' is not among the languages in use, so excluding it changed nothing.")));
    }

    reported
}

// Asked once, and not inside the map building, which runs once per kind of identity and per scope
// and is handed the same pairs every time.
fn find_unknown_forced_languages(by_name: &HashMap<String, Language>,
        forced: &ForcedLanguages) -> Vec<Warning>
{
    let mut names = by_name.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();

    let written = forced.to_written_form();
    let mut unknown = written.iter()
            .filter(|(_, wanted)| find_language_named(&names, wanted).is_none())
            .collect::<Vec<_>>();
    unknown.sort();

    unknown.into_iter().map(|(claimed, wanted)| Warning::new(warnings::Code::UnknownForcedLanguage, claimed,
            format!("Nothing called '{wanted}' is among the languages in use, so '{claimed}' was left as it was.")))
            .collect()
}

// With both halves written the same, Smalltalk's '"like this"', every position holds an opening and
// a closing at once and no two of them are ever paired. The pair goes and the language stays.
fn drop_the_pairs_that_never_fire(languages: &mut [Language]) -> Vec<Warning> {
    let mut reported = Vec::new();
    for language in languages.iter_mut() {
        for pairs in [&mut language.multiline_comments, &mut language.nesting_comments] {
            pairs.retain(|(start, end)| {
                if start != end {
                    return true;
                }
                reported.push(Warning::new(warnings::Code::CommentPairNeverCloses, &language.name,
                        format!("'{}' opens and closes a block comment with the same '{start}', which \
cannot be told apart, so that pair was dropped and its comments are counted as code.", language.name)));
                false
            });
        }
    }

    reported
}

// The file parser refuses both of these, so what this catches is a caller building one by hand.
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

// Asked against exactly the two maps the section lookup will consult, so the check and what it
// predicts cannot drift.
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

/// A warning for every name two or more languages carry, since only one of them can be in play.
// Which one survives is whatever order the directory was read in, so renaming a file changes the
// counts. Grouped by folded case, since 'Rust' and 'rust' are one language to every command that
// takes a name.
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

/// A warning for every language whose every extension, file name and `#!` name went to another one.
pub fn find_languages_that_lost_every_claim(languages: &[Language], conflicts: &ConflictRules) -> Vec<Warning> {
    let by_name = keyed_by_name(languages.to_vec());
    let (nothing_forced, no_rules) = (HashMap::new(), HashMap::new());
    let winners = [(IdentifiedBy::Extension, &conflicts.by_extension),
            (IdentifiedBy::Filename, &conflicts.by_filename),
            (IdentifiedBy::Shebang, &no_rules)].into_iter()
            .flat_map(|(identified_by, rules)|
                    build_language_map_by(identified_by, &by_name, rules, &nothing_forced).0.into_values())
            .collect::<HashSet<Arc<str>>>();

    let mut lost = languages.iter()
            .filter(|language| !language.extensions.is_empty() || !language.filenames.is_empty()
                    || !language.shebangs.is_empty())
            .filter(|language| !winners.contains(language.name.as_str()))
            // One that declares identification can still win files by their content
            .filter(|language| !language.declares_identification())
            .map(|language| language.name.as_str())
            .collect::<Vec<_>>();
    lost.sort();
    lost.dedup();

    lost.into_iter().map(|name| Warning::new(warnings::Code::LanguageLostEveryClaim, name,
            format!("'{name}' is installed, and every extension and name it claims belongs to another \
language, so no file can be counted as it."))).collect()
}

fn retain_languages_of_interest(languages: Vec<Language>, extensions: &HashMap<String, Arc<str>>,
        of_interest: &[String], excluded: &[String]) -> Vec<Language>
{
    // Ownership is read from the map the counting itself uses, so '--languages m' means the same
    // language every '.m' file is counted as here, however that was settled.
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
    fn a_contest_reaches_the_contenders_map_only_with_evidence_and_never_once_forced() {
        let build = |with_evidence: bool, forced: &HashMap<String, String>| {
            let mut languages = languages_claiming(&[("Alpha", &["x"]), ("Zed", &["x"]), ("Solo", &["y"])]);
            if with_evidence {
                languages.get_mut("Zed").unwrap().identifying_line_starts = vec!["zeddoc".to_owned()];
            }
            let (_, report) = crate::engine::identity::build_extension_language_map(
                    &languages, &HashMap::new(), forced);
            find_contested_with_evidence(&report, &languages)
        };

        let nothing_forced = HashMap::new();
        let contested = build(true, &nothing_forced);
        let contenders = contested.get("x").expect("the one contest with evidence is missing");
        assert_eq!(vec!["Alpha".to_owned(), "Zed".to_owned()],
                contenders.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
                "the fallback winner must come first, so evidence is asked in the standing order");
        assert_eq!(1, contested.len(), "an uncontested extension grew contenders");

        assert!(build(false, &nothing_forced).is_empty(),
                "a contest nobody declares evidence for still costs a lookup per file");
        let forced = hashmap!("x".to_owned() => "Zed".to_owned());
        assert!(build(true, &forced).is_empty(),
                "a forced extension is settled, and content must not overrule the person");
    }

    #[test]
    fn a_language_that_lost_every_contest_is_named_and_one_that_kept_a_claim_is_not() {
        let languages = languages_claiming(&[("Winner", &["x", "y"]), ("Loser", &["x"]),
                ("Halfway", &["y", "z"]), ("Alone", &["q"])]).into_values().collect::<Vec<_>>();
        let conflicts = ConflictRules {
            by_extension: hashmap!("x".to_owned() => vec!["Winner".to_owned(), "Loser".to_owned()],
                    "y".to_owned() => vec!["Winner".to_owned(), "Halfway".to_owned()]),
            ..Default::default()
        };

        let named = |warnings: Vec<Warning>| warnings.into_iter().map(|x| x.subject).collect::<Vec<_>>();
        assert_eq!(vec!["Loser".to_owned()], named(find_languages_that_lost_every_claim(&languages, &conflicts)),
                "'Halfway' kept '.z' and 'Alone' was never contested, so neither is unreachable");
        assert_eq!(warnings::Code::LanguageLostEveryClaim,
                find_languages_that_lost_every_claim(&languages, &conflicts)[0].code);

        let handed_back = ConflictRules {
            by_extension: hashmap!("x".to_owned() => vec!["Loser".to_owned(), "Winner".to_owned()],
                    "y".to_owned() => vec!["Winner".to_owned(), "Halfway".to_owned()]),
            ..Default::default()
        };
        assert!(find_languages_that_lost_every_claim(&languages, &handed_back).is_empty(),
                "'Winner' still holds '.y', so reordering '.x' leaves nobody unreachable");
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

    // Smalltalk's '"like this"'.
    #[test]
    fn a_comment_pair_written_the_same_at_both_ends_is_dropped_and_reported() {
        let mut languages = vec![
            Language::new("Smalltalky", ["stk"], StringRules::escaping_nothing(), [""; 0],
                    &[("\"", "\""), ("/*", "*/")], []),
            Language::new("Rust", ["rs"], StringRules::escaping_nothing(), ["//"], &[("/*", "*/")], [])
                    .with_nesting_comments(&[("/+", "/+")])];

        let reported = drop_the_pairs_that_never_fire(&mut languages);

        assert_eq!(vec![("/*".to_owned(), "*/".to_owned())], languages[0].multiline_comments,
                "the pair that works was taken away with the one that does not");
        assert_eq!(vec![("/*".to_owned(), "*/".to_owned())], languages[1].multiline_comments);
        assert!(languages[1].nesting_comments.is_empty(), "a nesting pair is refused on the same rule");

        assert_eq!(2, reported.len(), "{reported:?}");
        assert!(reported.iter().all(|x| x.code == warnings::Code::CommentPairNeverCloses), "{reported:?}");
        assert_eq!(vec!["Smalltalky", "Rust"], reported.iter().map(|x| x.subject.as_str()).collect::<Vec<_>>());
        assert_eq!(warnings::Affects::Counts, reported[0].affects(),
                "the comments of that language land in the code, so the numbers moved");
    }

    // What a definition for Makefile or Dockerfile alone looks like: filenames and no extension.
    #[test]
    fn a_language_that_claims_only_filenames_is_kept() {
        let by_name_only = Language::new("Docky", [""; 0], StringRules::escaping_nothing(), ["#"], &[], [])
                .with_filenames(["Dockerfile"]);

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
                .with_filenames(["SConstruct"])];

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

    // Without the shipped rules, 'm' goes to Objective-C alphabetically and a MATLAB file is
    // counted as Objective-C.
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
