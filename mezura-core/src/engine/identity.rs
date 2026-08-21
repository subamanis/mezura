// Which language owns an extension, a whole filename or a shebang interpreter, and how a contest
// between two of them is settled.
use std::{collections::HashMap, fs::File, io::ErrorKind, io::Read, path::Path, sync::Arc};

use crate::{Language, warnings};

// Longer than any extension that exists and than the filenames anybody writes, and the buffer that
// keeps the case-insensitive lookup from allocating once per file
const MAX_IDENTITY_LEN : usize = 32;

// One read this size answers the probe, so an extensionless binary costs a single small read
const SHEBANG_READ_LIMIT : usize = 256;

// The three ways a file says which language it is. They go through the same contest machinery and
// differ in one thing each: what a language declares, and how the text is keyed.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum IdentifiedBy {
    Extension,
    Filename,
    Shebang
}

impl IdentifiedBy {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::Filename => "filename",
            Self::Shebang => "shebang"
        }
    }

    fn declared_by<'a>(&self, language: &'a Language) -> &'a [String] {
        match self {
            Self::Extension => &language.extensions,
            Self::Filename => &language.filenames,
            Self::Shebang => &language.shebangs
        }
    }

    // A filename keeps its dots, since '.gitignore' is a name and not an extension of nothing
    fn key_of(&self, text: &str) -> String {
        match self {
            Self::Extension => extension_key(text),
            Self::Filename | Self::Shebang => text.to_ascii_lowercase()
        }
    }
}

// The three outcomes must never read alike: the first two are decisions somebody took, the third is
// a tiebreak nobody asked for and the one that can put a language's comments into another's code.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum ResolvedBy {
    ForceLang,
    PriorityFile,
    AlphabeticalFallback
}

// One text more than one language claims, and how it was settled.
#[derive(Debug,PartialEq,Eq,Clone)]
#[non_exhaustive]
pub struct ContestedIdentity {
    pub identity: String,
    pub identified_by: IdentifiedBy,
    pub winner: String,
    pub losers: Vec<String>,
    pub resolved_by: ResolvedBy
}

#[derive(Debug,PartialEq,Eq,Clone,Default)]
#[non_exhaustive]
pub struct IdentityReport {
    pub contested: Vec<ContestedIdentity>
}

impl IdentityReport {
    // Only the alphabetical tiebreak is reported. A rule or a forced pair is somebody's own decision,
    // and saying so every run buries the one line that matters. One warning per extension, so
    // whoever reads the document can key on it.
    //
    // Each says what happened and stops. What to do about it depends on who is calling: the command
    // line has a file and a flag for it and adds its own sentence, a library caller has neither.
    pub fn collect_warnings(&self) -> Vec<warnings::Warning> {
        let mut reported = Vec::new();
        for contested in self.contested.iter().filter(|x| x.resolved_by == ResolvedBy::AlphabeticalFallback) {
            reported.push(warnings::Warning::new(warnings::Code::LanguageTiebreak, &contested.identity,
                    format!("The {} '{}' is claimed by {} and {}. It was given to {} only because that name comes first \
alphabetically, so the files of the rest are counted with the wrong comment and string symbols.",
                    contested.identified_by.name(), contested.identity, contested.winner, contested.losers.join(", "),
                    contested.winner)));
        }

        reported
    }
}

// Keys are lowercased here, once, and the lookup lowercases what it is given. Before the claimants
// are counted, not after: left as written, 'cs' and 'CS' look like two extensions, never collide,
// and each wins silently in different files.
pub fn build_language_map_by(identified_by: IdentifiedBy, languages: &HashMap<String,Language>,
        priority: &HashMap<String,Vec<String>>, forced: &HashMap<String,String>)
        -> (HashMap<String, Arc<str>>, IdentityReport)
{
    let mut names = languages.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();

    let shared_names : HashMap<&str, Arc<str>> = names.iter()
            .map(|name| (*name, Arc::from(*name)))
            .collect();

    // Normalised once, so the two places that consult it cannot disagree about the shape of a key.
    // A library caller sets this field directly and owes nothing about case or the leading dot, and
    // when only one of the two lookups folded them, the mapping was applied while the run warned in
    // the same breath that the extension had been left to the tiebreak.
    let forced : HashMap<String, &str> = forced.iter()
            .map(|(identity, language)| (identified_by.key_of(identity), language.as_str()))
            .collect();
    let language_named = |wanted: &str| find_language_named(&names, wanted);

    let mut claimants : HashMap<String, Vec<&str>> = HashMap::with_capacity(languages.len() * 2);
    for name in &names {
        for declared in identified_by.declared_by(&languages[*name]) {
            let claiming = claimants.entry(identified_by.key_of(declared)).or_default();
            // A language claiming one extension twice, as 'h' and '.h', is not a contest. Left in, it
            // becomes its own rival: the collision fires, every loser equals the winner and is
            // filtered out, and the warning reads "claimed by Cish and ." Dropped in silence because
            // the counts were right and there is nothing for the reader to fix.
            if !claiming.contains(name) {
                claiming.push(name);
            }
        }
    }

    let mut map : HashMap<String, Arc<str>> = HashMap::with_capacity(claimants.len());
    let mut report = IdentityReport::default();

    for (identity, claimants) in claimants {
        let forced_winner = forced.get(&identity).and_then(|wanted| language_named(wanted));
        // Exact before folded, for the same reason as the lookup above: a rule of the priority file
        // naming one of two spellings has no other way to say which it meant.
        let priority_winner = priority.get(&identity)
                .and_then(|order| order.iter()
                        .find_map(|wanted| claimants.iter().find(|name| **name == wanted.as_str())
                                .or_else(|| claimants.iter().find(|name| crate::languages::is_the_same_language_name(name, wanted))))
                        .copied());

        // The winner and the mechanism that chose it are decided in one place, because deriving the
        // second from "is there a rule for this extension" is not the same question. A rule naming a
        // language that does not claim the extension, because it was renamed, removed or misspelled,
        // settles nothing: the tiebreak decides, and reporting it as settled hides exactly the case
        // this whole mechanism exists to announce.
        // The claimants were pushed in the order the sorted names were walked, so the first of them
        // is the alphabetical winner this has always fallen back to.
        let (winner, resolved_by) = match (forced_winner, priority_winner) {
            (Some(x), _) => (x, ResolvedBy::ForceLang),
            (_, Some(x)) => (x, ResolvedBy::PriorityFile),
            _ => (claimants[0], ResolvedBy::AlphabeticalFallback)
        };

        if claimants.len() > 1 {
            report.contested.push(ContestedIdentity {
                identity: identity.clone(),
                identified_by,
                winner: winner.to_owned(),
                losers: claimants.iter().filter(|name| **name != winner).map(|name| (*name).to_owned()).collect(),
                resolved_by
            });
        }

        map.insert(identity, shared_names[winner].clone());
    }

    // '--force-language txt=python' is meant to work whether or not anything else claims the extension,
    // so a forced entry that no language claims is added rather than ignored. A pair naming a language
    // that does not exist is not complained about here, because this runs once per kind of identity
    // and the same pair is given to every one of them: 'Languages::resolve' asks that once.
    // The shebang map is the exception: a forced extension is not an interpreter, and one unclaimed
    // entry here would turn the probe on for a run whose languages declare no shebang at all. A pair
    // naming an interpreter some language does claim has already won above.
    if identified_by != IdentifiedBy::Shebang {
        for (identity, wanted) in &forced {
            if let Some(name) = language_named(wanted) {
                map.insert(identity.clone(), shared_names[name].clone());
            }
        }
    }

    report.contested.sort_by(|a, b| a.identity.cmp(&b.identity));
    (map, report)
}

// The maps a run counts with, and the one question a file is asked. Together because the answer
// is one answer: a file whose whole name is claimed is that language whatever its extension says,
// which is what makes 'CMakeLists.txt' CMake rather than text.
#[derive(Debug, Default)]
pub struct LanguageLookup {
    pub by_extension: HashMap<String, Arc<str>>,
    pub by_filename: HashMap<String, Arc<str>>,
    pub by_shebang: HashMap<String, Arc<str>>
}

impl LanguageLookup {
    pub fn of_path(&self, path: &Path) -> Option<Arc<str>> {
        // The name is asked first and only when something claims one, so a run whose languages
        // declare no filename pays one branch and no lookup
        if !self.by_filename.is_empty()
                && let Some(name) = path.file_name().and_then(|x| x.to_str())
                && let Some(language) = find_language_of_identity(&self.by_filename, name) {
            return Some(language);
        }
        let extension = path.extension().and_then(|x| x.to_str())?;
        find_language_of_identity(&self.by_extension, extension)
    }

    // For a file named directly as a target, where nothing between the name lookup and the probe
    // gets to exclude it
    pub fn of_path_or_shebang(&self, path: &Path) -> Option<Arc<str>> {
        self.of_path(path).or_else(|| self.of_shebang(path))
    }

    // Asked before 'of_shebang' opens anything, so the walk can run its ignore checks in between
    // and a file nobody wants counted is never opened.
    pub fn needs_a_shebang_probe(&self, path: &Path) -> bool {
        // 'extension()' answers None for dotfiles too, so a '.bashrc'-shaped name qualifies. The
        // candidates stay few because '.git' is never walked and dotted directories are skipped
        // unless '--search-in-dotted' asks for them.
        !self.by_shebang.is_empty() && path.extension().is_none()
    }

    pub fn of_shebang(&self, path: &Path) -> Option<Arc<str>> {
        if !self.needs_a_shebang_probe(path) {
            return None;
        }
        let (buffer, length) = read_first_bytes(path)?;
        let mut line = &buffer[..length];
        // A full buffer with no line break means the first line was cut, and a cut word must
        // not be matched: 'rubyfmt' cut at 'ruby' would count as Ruby. Everything after the
        // last whitespace goes, and a line that is one unbroken word answers nothing.
        if length == SHEBANG_READ_LIMIT && !line.contains(&b'\n') {
            let last_space = line.iter().rposition(|b| matches!(b, b' ' | b'\t' | b'\r'))?;
            line = &line[..last_space];
        }
        let interpreter = std::str::from_utf8(find_interpreter(line)?).ok()?;
        find_language_of_interpreter(&self.by_shebang, interpreter)
    }
}

pub fn find_language_of_identity(language_of: &HashMap<String, Arc<str>>, identity: &str) -> Option<Arc<str>> {
    if let Some(x) = language_of.get(identity) {
        return Some(x.clone());
    }

    // Every key is already lowercase, so anything that is too, has simply not been found
    if !identity.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }

    // The buffer below is the hot path and covers every extension and filename anybody actually
    // writes. One longer than it comes from somebody's own language file, and is worth the
    // allocation rather than the silent miss it used to be: the file was simply never counted and
    // nothing said so.
    if identity.len() > MAX_IDENTITY_LEN {
        return language_of.get(&identity.to_ascii_lowercase()).cloned();
    }

    let mut buffer = [0u8; MAX_IDENTITY_LEN];
    let length = identity.len();
    buffer[..length].copy_from_slice(identity.as_bytes());
    buffer[..length].make_ascii_lowercase();
    std::str::from_utf8(&buffer[..length]).ok()
            .and_then(|lowercased| language_of.get(lowercased))
            .cloned()
}

// Searched in the sorted order the names are given in, and never through the keys of a map, whose
// iteration order is arbitrary: two languages whose names differ only in case would otherwise
// resolve to a different one of the two between runs of the same command.
//
// The exact spelling wins before case is folded, because folding first cannot be undone: with both
// 'Rust' and 'rust' declared, naming one of them got the other, the one whose capital sorts first,
// along with its comment symbols and without a word.
pub(crate) fn find_language_named<'a>(sorted_names: &[&'a str], wanted: &str) -> Option<&'a str> {
    sorted_names.iter().find(|name| **name == wanted)
            .or_else(|| sorted_names.iter().find(|name| crate::languages::is_the_same_language_name(name, wanted)))
            .copied()
}

// The interpreter a '#!' first line names: the last path component of its first word, or, when
// that is 'env', the first word after it that is neither a flag nor a NAME=value assignment,
// which is what lets '#!/usr/bin/env -S deno run' answer 'deno'.
pub(crate) fn find_interpreter(first_line: &[u8]) -> Option<&[u8]> {
    let line = first_line.strip_prefix(b"#!")?;
    let line = &line[..line.iter().position(|b| *b == b'\n').unwrap_or(line.len())];
    let mut words = line.split(|b| matches!(b, b' ' | b'\t' | b'\r')).filter(|word| !word.is_empty());
    let command = words.next()?;
    let command = command.rsplit(|b| *b == b'/').next().unwrap_or(command);
    if command != b"env" {
        return Some(command);
    }
    words.find(|word| !word.starts_with(b"-") && !word.contains(&b'='))
}

// Looped because one 'read' may legally return short, and an interrupted call is retried
// instead of being read as "no language".
fn read_first_bytes(path: &Path) -> Option<([u8; SHEBANG_READ_LIMIT], usize)> {
    let mut file = File::open(path).ok()?;
    let mut buffer = [0u8; SHEBANG_READ_LIMIT];
    let mut length = 0;
    while length < SHEBANG_READ_LIMIT {
        match file.read(&mut buffer[length..]) {
            Ok(0) => break,
            Ok(bytes) => length += bytes,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return None
        }
    }
    Some((buffer, length))
}

// The exact spelling first, then one version segment fewer at a time, so a declared 'python3'
// answers for 'python3.12' before 'python' can, and 'perl6.0.0' reaches a declared 'perl6'
// instead of falling through to Perl.
pub(crate) fn find_language_of_interpreter(language_of: &HashMap<String, Arc<str>>, interpreter: &str)
        -> Option<Arc<str>> {
    interpreter_spellings(interpreter).iter()
            .find_map(|spelling| language_of.get(spelling))
            .cloned()
}

// Every spelling the interpreter lookup tries, most specific first. Shared with the fixture
// test that mirrors the resolution, so the two cannot drift.
pub(crate) fn interpreter_spellings(interpreter: &str) -> Vec<String> {
    let mut spellings = vec![interpreter.to_ascii_lowercase()];
    let mut candidate = interpreter;
    loop {
        let trimmed = candidate.trim_end_matches(|c: char| c.is_ascii_digit());
        let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
        if trimmed.is_empty() || trimmed.len() == candidate.len() {
            return spellings;
        }
        spellings.push(trimmed.to_ascii_lowercase());
        candidate = trimmed;
    }
}

// The one spelling of an extension that everything keys on: no leading dot, lowercased the way the
// lookup lowercases what it is handed. Non-ASCII is left alone, since 'to_ascii_lowercase' is what
// the lookup uses and the two must agree on every byte.
pub(crate) fn extension_key(extension: &str) -> String {
    extension.trim_start_matches('.').to_ascii_lowercase()
}

pub(crate) fn identity_key(identified_by: IdentifiedBy, text: &str) -> String {
    identified_by.key_of(text)
}

// Every test here and in the walk is about extensions, and naming the side each time reads as though
// the choice mattered to what is being asserted.
#[cfg(test)]
pub fn build_extension_language_map(languages: &HashMap<String,Language>, priority: &HashMap<String,Vec<String>>,
        forced: &HashMap<String,String>) -> (HashMap<String, Arc<str>>, IdentityReport)
{
    build_language_map_by(IdentifiedBy::Extension, languages, priority, forced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages_claiming;

    // Three places turn an extension into a key: a language's own declaration, a '--force-language'
    // pair, and a rule of the priority file. Only the forced one used to strip a leading dot, so a
    // language declaring '.dot' claimed nothing and a rule written '.m' settled nothing, both in
    // silence. The dotted form is what every editor and every other counter writes.
    #[test]
    fn an_extension_is_keyed_the_same_way_wherever_it_is_declared() {
        let dotted = languages_claiming(&[("Dotty", &[".dot"])]);
        let (map, _) = build_extension_language_map(&dotted, &HashMap::new(), &HashMap::new());
        assert_eq!(Some("Dotty"), map.get("dot").map(|x| x.as_ref()),
                "a language declaring '.dot' claims nothing: {map:?}");

        // and a rule of the priority file reaches the same key
        let (rules, faulty) = crate::language_file::parse_priority(
                "===> contested-extensions\n.m       MATLAB, Objective-C\n");
        assert!(faulty.is_empty());
        assert_eq!(Some(&vec!["MATLAB".to_owned(), "Objective-C".to_owned()]), rules.by_extension.get("m"));

        let contested = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &[".m"])]);
        let (map, report) = build_extension_language_map(&contested, &rules.by_extension, &HashMap::new());
        assert_eq!(Some("MATLAB"), map.get("m").map(|x| x.as_ref()));
        assert_eq!(1, report.contested.len(), "one declared with a dot and one without did not meet");
        assert_eq!(ResolvedBy::PriorityFile, report.contested[0].resolved_by);
    }

    // The stack buffer is sized for every extension that exists today, and anything longer with a
    // capital in it used to be given up on rather than lowercased, so the files were not counted
    // and nothing said so.
    #[test]
    fn an_extension_longer_than_the_buffer_is_still_matched_case_insensitively() {
        let long = "A".repeat(MAX_IDENTITY_LEN + 6);
        let languages = languages_claiming(&[("Longy", &[long.as_str()])]);
        let (map, _) = build_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!(Some("Longy"), find_language_of_identity(&map, &long.to_lowercase()).as_deref());
        assert_eq!(Some("Longy"), find_language_of_identity(&map, &long).as_deref(),
                "an extension of {} bytes was given up on instead of lowercased", long.len());
        // and one that is genuinely absent is still absent, whatever its length
        assert_eq!(None, find_language_of_identity(&map, &"B".repeat(MAX_IDENTITY_LEN + 6)));
    }

    fn priority(rules: &[(&str, &[&str])]) -> HashMap<String,Vec<String>> {
        rules.iter().map(|(extension, order)| ((*extension).to_owned(),
                order.iter().map(|x| (*x).to_owned()).collect())).collect()
    }

    fn winner_of(map: &HashMap<String, Arc<str>>, extension: &str) -> String {
        find_language_of_identity(map, extension).map(|x| x.to_string()).unwrap_or_default()
    }

    #[test]
    fn an_extension_that_only_one_language_claims_is_never_reported() {
        let languages = languages_claiming(&[("Rust", &["rs"]), ("Go", &["go"])]);
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &HashMap::new());
    
        assert_eq!("Rust", winner_of(&map, "rs"));
        assert_eq!("Go", winner_of(&map, "go"));
        assert_eq!(IdentityReport::default(), report);
        assert!(report.collect_warnings().is_empty());
    }

    // The tiebreak is the outcome nobody chose, and the only one that is announced
    #[test]
    fn a_contested_extension_falls_back_to_the_first_name_alphabetically_and_says_so() {
        let languages = languages_claiming(&[("Objective-C", &["m", "mm"]), ("MATLAB", &["m"])]);
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &HashMap::new());
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!("Objective-C", winner_of(&map, "mm"));
        assert_eq!(vec![ContestedIdentity {
            identity: "m".to_owned(),
            identified_by: IdentifiedBy::Extension,
            winner: "MATLAB".to_owned(),
            losers: vec!["Objective-C".to_owned()],
            resolved_by: ResolvedBy::AlphabeticalFallback
        }], report.contested);
        assert_eq!(vec![(warnings::Code::LanguageTiebreak, "counts")],
                report.collect_warnings().iter().map(|x| (x.code, x.affects().name())).collect::<Vec<_>>());
    }

    // One language, two spellings of one extension, which stopped being two keys the moment the
    // leading dot began to be stripped. The contest machinery then fired on a language against
    // itself: the winner was filtered out of its own list of losers, leaving none, and the sentence
    // came out as "claimed by Cish and ." filed against the counts, which were never in question.
    // The dotted form is the one the comments here call what every editor writes, so a user moving
    // their file over to it and leaving the bare one behind is the ordinary way in.
    #[test]
    fn a_language_claiming_one_extension_twice_does_not_contest_it_with_itself() {
        let languages = languages_claiming(&[("Cish", &["h", ".h", "H"])]);
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!("Cish", winner_of(&map, "h"));
        assert_eq!(1, map.len(), "the three spellings did not fold into one key");
        assert!(report.contested.is_empty(), "a language was reported as contesting itself: {:?}", report.contested);
        assert!(report.collect_warnings().is_empty(), "{:?}", report.collect_warnings());

        // and a real contest over the same extension is still announced, so what is gone is the
        // self-collision and not the check
        let contested = languages_claiming(&[("Cish", &[".h"]), ("Bish", &["h"])]);
        let (_, report) = build_extension_language_map(&contested, &HashMap::new(), &HashMap::new());
        assert_eq!(vec!["Cish".to_owned()], report.contested[0].losers);
    }

    #[test]
    fn the_priority_file_decides_it_and_force_lang_overrules_the_priority_file() {
        let languages = languages_claiming(&[("Objective-C", &["m"]), ("MATLAB", &["m"])]);
    
        let (map, report) = build_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &HashMap::new());
        assert_eq!("Objective-C", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::PriorityFile, report.contested[0].resolved_by);
        assert_eq!(vec!["MATLAB".to_owned()], report.contested[0].losers);
    
        let forced = hashmap!("m".to_owned() => "matlab".to_owned());
        let (map, report) = build_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &forced);
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.contested[0].resolved_by);
    
        // and neither of them is the tiebreak, so neither is announced
        assert!(report.collect_warnings().is_empty());
    }

    // A rule whose every name has been renamed away, removed or misspelled settles nothing, and the
    // tiebreak is what decides. Reporting it as settled left the user believing their rule was in
    // force while the extension quietly went elsewhere, with nothing printed.
    #[test]
    fn a_priority_rule_that_names_no_claimant_falls_through_to_the_tiebreak_and_says_so() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let (map, report) = build_extension_language_map(&languages, &priority(&[("m", &["ObjC"])]), &HashMap::new());
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::AlphabeticalFallback, report.contested[0].resolved_by);
        let reported = report.collect_warnings();
        assert_eq!(warnings::Code::LanguageTiebreak, reported[0].code);
        assert_eq!("m", reported[0].subject);
        assert!(reported[0].message.contains("only because"));
    }

    // A name in the priority file that no longer exists is skipped rather than left to win nothing
    #[test]
    fn the_priority_file_moves_on_to_the_next_name_when_the_first_is_not_there() {
        let languages = languages_claiming(&[("Prolog", &["pl"]), ("Raku", &["pl"])]);
        let (map, _) = build_extension_language_map(&languages, &priority(&[("pl", &["Perl", "Raku", "Prolog"])]), &HashMap::new());
    
        assert_eq!("Raku", winner_of(&map, "pl"));
    }

    #[test]
    fn a_forced_extension_is_taken_even_when_no_language_claims_it() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("txt".to_owned() => "python".to_owned());
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &forced);
    
        assert_eq!("Python", winner_of(&map, "txt"));
        assert_eq!("Python", winner_of(&map, "py"));
        // nothing was contested, so there is nothing to report
        assert!(report.contested.is_empty());
    }

    // A caller of the library sets the field directly and is under no obligation to lowercase its
    // keys. When only the second of the two lookups normalised, the mapping was applied and the run
    // warned in the same breath that the extension had been left to the alphabetical tiebreak.
    #[test]
    fn a_forced_extension_is_normalised_before_it_is_looked_up() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let forced = hashmap!("M".to_owned() => "MatLab".to_owned());
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &forced);
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.contested[0].resolved_by);
        assert!(report.collect_warnings().is_empty());
    }

    // The complaint about the name belongs to 'Languages::resolve', which asks it once for every map;
    // what this map owes is that the pair changed nothing.
    #[test]
    fn a_forced_language_that_is_not_available_changes_nothing_and_is_left_to_be_reported() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("py".to_owned() => "cobol".to_owned());
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &forced);

        assert_eq!("Python", winner_of(&map, "py"));
        assert!(report.contested.is_empty());
        assert!(report.collect_warnings().is_empty());
    }

    // Two spellings of one extension are one extension, and they have to collide as one. Left as
    // they were written they would look like two, would never be found to contest anything, and
    // would each quietly win in the files that happened to be spelled their way.
    #[test]
    fn extensions_are_matched_without_case_and_contest_each_other_across_it() {
        let languages = languages_claiming(&[("Zig", &["ZIG"]), ("Ziggy", &["zig"])]);
        let (map, report) = build_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!(1, report.contested.len());
        assert_eq!("zig", report.contested[0].identity);
        assert_eq!("Zig", winner_of(&map, "zig"));
        assert_eq!("Zig", winner_of(&map, "ZIG"));
        assert_eq!("Zig", winner_of(&map, "Zig"));
        assert_eq!("", winner_of(&map, "zigg"));
    }

    fn interpreter_of(line: &str) -> Option<String> {
        find_interpreter(line.as_bytes()).map(|x| String::from_utf8_lossy(x).into_owned())
    }

    // The whitespace and flag matrix scc settled in its issue 115, which is the most field-tested
    // set of first lines there is, plus the 'env -S' and NAME=value forms that only linguist
    // handles and this parser takes from it.
    #[test]
    fn the_interpreter_is_read_out_of_every_shape_a_shebang_line_takes() {
        for line in ["#!/usr/bin/perl", "#!  /usr/bin/perl", "#!/usr/bin/perl -w", "#!/usr/bin/env perl",
                "#!  /usr/bin/env   perl", "#!/usr/bin/env perl -w", "#!  /usr/bin/env   perl   -w  ",
                "#!/opt/local/bin/perl", "#!perl", "#! perl", "#!/usr/bin/perl\nprint 1;"] {
            assert_eq!(Some("perl".to_owned()), interpreter_of(line), "on {line:?}");
        }
        assert_eq!(Some("perl5".to_owned()), interpreter_of("#!/usr/bin/perl5"),
                "the version is the lookup's business, not the line parser's");

        // 'env' takes flags and variable assignments before the interpreter, and a flag cluster
        // is one word: all of these run in the wild
        assert_eq!(Some("python3".to_owned()), interpreter_of("#!/usr/bin/env -S python3"));
        assert_eq!(Some("deno".to_owned()), interpreter_of("#!/usr/bin/env -S deno run --allow-read"));
        assert_eq!(Some("python".to_owned()), interpreter_of("#!/usr/bin/env -vS PYTHONPATH=/opt python"));
        assert_eq!(Some("bash".to_owned()), interpreter_of("#!/usr/bin/env --split-string bash"));

        // and a windows checkout hands the line over with its '\r' still on
        assert_eq!(Some("sh".to_owned()), interpreter_of("#!/bin/sh\r\necho hi"));
    }

    #[test]
    fn a_line_that_names_no_interpreter_answers_nothing() {
        // no '#!' first, which is also what any binary or BOM-carrying file looks like: the kernel
        // itself refuses a shebang behind a BOM, so such a file is not a script
        assert_eq!(None, interpreter_of("echo hi"));
        assert_eq!(None, interpreter_of("\u{feff}#!/bin/sh"));
        assert_eq!(None, interpreter_of(""));
        // a '#!' with nothing usable after it
        assert_eq!(None, interpreter_of("#!"));
        assert_eq!(None, interpreter_of("#!/usr/bin/env"));
        assert_eq!(None, interpreter_of("#!/usr/bin/env -S"));
    }

    fn shebang_lookup(claims: &[(&str, &[&str])]) -> LanguageLookup {
        let languages = claims.iter()
                .map(|(name, interpreters)| ((*name).to_owned(),
                        Language::new(*name, ["zzz"], ["\""], ["#"], &[], []).with_shebangs(interpreters)))
                .collect();
        LanguageLookup {
            by_shebang: build_language_map_by(IdentifiedBy::Shebang, &languages,
                    &HashMap::new(), &HashMap::new()).0,
            ..Default::default()
        }
    }

    // A declared 'python' answers for 'python3' and 'python3.12' without every version being
    // enumerated, and the exact match goes first so 'perl6', which is Raku and not a Perl
    // version, goes where it was declared.
    #[test]
    fn a_versioned_interpreter_falls_back_to_its_declared_name_and_an_exact_claim_wins_first() {
        let root = std::env::temp_dir().join("mezura_shebang_versions_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = |name: &str, first_line: &str| {
            let path = root.join(name);
            std::fs::write(&path, format!("{first_line}\nbody\n")).unwrap();
            path
        };
        let lookup = shebang_lookup(&[("Python", &["python"]), ("Perl", &["perl"]),
                ("Raku", &["perl6"]), ("R", &["Rscript"])]);

        assert_eq!(Some("Python"), lookup.of_shebang(&script("versioned", "#!/usr/bin/python3.12")).as_deref());
        assert_eq!(Some("Python"), lookup.of_shebang(&script("enved", "#!/usr/bin/env python3")).as_deref());
        assert_eq!(Some("Raku"), lookup.of_shebang(&script("raku", "#!/usr/bin/perl6")).as_deref());
        assert_eq!(Some("Perl"), lookup.of_shebang(&script("versioned-perl", "#!/usr/bin/perl5.36")).as_deref());
        // matched the way every other identity is, without case
        assert_eq!(Some("R"), lookup.of_shebang(&script("rscript", "#!/usr/bin/env Rscript")).as_deref());
        // an interpreter that is nothing but digits after the trim is not a match for everything
        assert_eq!(None, lookup.of_shebang(&script("digits", "#!/usr/bin/386")).as_deref());

        std::fs::remove_dir_all(&root).unwrap();
    }

    // The trim walks back one version segment at a time, so the most specific declared spelling
    // wins: in one step, 'python3.12' jumped straight to 'python' past a declared 'python3',
    // and 'perl6.0.0' turned a Raku script into Perl.
    #[test]
    fn the_version_fallback_stops_at_the_most_specific_declared_spelling() {
        let root = std::env::temp_dir().join("mezura_shebang_segments_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = |name: &str, first_line: &str| {
            let path = root.join(name);
            std::fs::write(&path, format!("{first_line}\nbody\n")).unwrap();
            path
        };
        let lookup = shebang_lookup(&[("OldPython", &["python"]), ("NewPython", &["python3"]),
                ("Perl", &["perl"]), ("Raku", &["perl6"])]);

        assert_eq!(Some("NewPython"), lookup.of_shebang(&script("py312", "#!/usr/bin/python3.12")).as_deref());
        assert_eq!(Some("OldPython"), lookup.of_shebang(&script("py27", "#!/usr/bin/python2.7")).as_deref());
        assert_eq!(Some("Raku"), lookup.of_shebang(&script("raku", "#!/usr/bin/perl6.0.0")).as_deref());
        assert_eq!(Some("Perl"), lookup.of_shebang(&script("perl", "#!/usr/bin/perl5.36.0")).as_deref());

        std::fs::remove_dir_all(&root).unwrap();
    }

    // A first line longer than the probe's window arrives cut, and a cut word is not an
    // interpreter: 'rubyfmt' cut at 'ruby' counted as Ruby until this was guarded.
    #[test]
    fn a_first_line_longer_than_the_probe_window_never_matches_a_cut_word() {
        let root = std::env::temp_dir().join("mezura_shebang_cut_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = |name: &str, first_line: &str| {
            let path = root.join(name);
            std::fs::write(&path, format!("{first_line}\nbody\n")).unwrap();
            path
        };
        let lookup = shebang_lookup(&[("Ruby", &["ruby"]), ("Shell", &["bash"])]);

        // sized so the window ends right after the 'ruby' inside 'rubyfmt'
        let padding = "a".repeat(SHEBANG_READ_LIMIT - "#!/usr/bin/env -S F= ruby".len());
        let cut_mid_word = script("cut", &format!("#!/usr/bin/env -S F={padding} rubyfmt --check"));
        assert_eq!(None, lookup.of_shebang(&cut_mid_word).as_deref());

        // an interpreter that fits inside the window still answers, whatever runs past it
        let cut_in_arguments = script("late-cut", &format!("#!/bin/bash {}", "x".repeat(400)));
        assert_eq!(Some("Shell"), lookup.of_shebang(&cut_in_arguments).as_deref());

        // and one unbroken word filling the window answers nothing rather than a prefix of itself
        let one_word = script("one-word", &format!("#!/usr/bin/ruby{}", "y".repeat(400)));
        assert_eq!(None, lookup.of_shebang(&one_word).as_deref());

        std::fs::remove_dir_all(&root).unwrap();
    }

    // A forced pair whose identity nothing claims lands in the extension and filename maps and
    // deliberately not in the shebang one: there it would turn the probe on for a run whose
    // languages declare no shebang at all, and let an extension spelling answer as an interpreter.
    #[test]
    fn a_forced_pair_nothing_claims_stays_out_of_the_shebang_map() {
        let languages = languages_claiming(&[("Rust", &["rs"])]);
        let forced = hashmap!("txt".to_owned() => "rust".to_owned());
        let (map, _) = build_language_map_by(IdentifiedBy::Shebang, &languages, &HashMap::new(), &forced);
        assert!(map.is_empty(), "a forced extension became an interpreter: {map:?}");

        // and the same pair still settles a real interpreter contest
        let contested: HashMap<String, Language> = [("Ash", ["sh"]), ("Bsh", ["sh"])].into_iter()
                .map(|(name, interpreters)| (name.to_owned(),
                        Language::new(name, ["zzz"], ["\""], ["#"], &[], []).with_shebangs(&interpreters)))
                .collect();
        let forced = hashmap!("sh".to_owned() => "bsh".to_owned());
        let (map, report) = build_language_map_by(IdentifiedBy::Shebang, &contested, &HashMap::new(), &forced);
        assert_eq!(Some("Bsh"), map.get("sh").map(|x| x.as_ref()));
        assert_eq!(ResolvedBy::ForceLang, report.contested[0].resolved_by);
    }

    // The probe's bound is the whole of its cost: only a file with no extension at all is ever
    // opened, whatever its first line would have said.
    #[test]
    fn only_an_extensionless_file_is_probed_and_a_probe_that_finds_nothing_claims_nothing() {
        let root = std::env::temp_dir().join("mezura_shebang_probe_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = |name: &str, contents: &str| {
            let path = root.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        };
        let lookup = shebang_lookup(&[("Shell", &["sh", "bash"])]);

        let deploy = script("deploy", "#!/bin/bash\necho hi\n");
        assert!(lookup.needs_a_shebang_probe(&deploy));
        assert_eq!(Some("Shell"), lookup.of_shebang(&deploy).as_deref());
        assert_eq!(Some("Shell"), lookup.of_path_or_shebang(&deploy).as_deref());

        // an extension, even one nothing claims, keeps the file out of the probe
        let with_extension = script("deploy.xyz", "#!/bin/bash\necho hi\n");
        assert!(!lookup.needs_a_shebang_probe(&with_extension));
        assert_eq!(None, lookup.of_shebang(&with_extension));

        // a first line that is not a shebang, and a binary-shaped one, both stay unclaimed
        assert_eq!(None, lookup.of_shebang(&script("LICENSE", "MIT License\n")));
        assert_eq!(None, lookup.of_shebang(&script("compiled", "\u{0}\u{1}binary#!/bin/sh")));
        // an interpreter nobody declares stays unclaimed too
        assert_eq!(None, lookup.of_shebang(&script("looped", "#!/usr/bin/lua\nprint(1)\n")));

        // with no language declaring a shebang, nothing qualifies and nothing is opened
        let no_shebangs = LanguageLookup::default();
        assert!(!no_shebangs.needs_a_shebang_probe(&deploy));
        assert_eq!(None, no_shebangs.of_shebang(&deploy));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
