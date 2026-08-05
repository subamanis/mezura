// Which language owns an extension, and how a contest between two of them is settled.
use std::{collections::HashMap, sync::Arc};

use crate::{EXTENSION_PRIORITY_FILE_NAME, Language, warnings};

// Longer than any extension that exists, and the buffer that keeps the case-insensitive lookup from
// allocating once per file
const MAX_EXTENSION_LEN : usize = 24;

// An extension claimed by more than one language, and how that was settled. The three outcomes are
// not equally trustworthy and must never read alike: the first two are decisions somebody took, the
// third is a tiebreak nobody asked for, and it is the one that can put a language's comments into
// another language's 'code'.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum ResolvedBy {
    ForceLang,
    PriorityFile,
    AlphabeticalFallback
}

#[derive(Debug,PartialEq,Eq,Clone)]
#[non_exhaustive]
pub struct ExtensionCollision {
    pub extension: String,
    pub winner: String,
    pub losers: Vec<String>,
    pub resolved_by: ResolvedBy
}

#[derive(Debug,PartialEq,Eq,Clone,Default)]
#[non_exhaustive]
pub struct ExtensionReport {
    pub collisions: Vec<ExtensionCollision>,
    pub unknown_forced_languages: Vec<(String,String)>
}

impl ExtensionReport {
    // Only the tiebreak is reported. A collision that the priority file or '--force-lang' settled is
    // a decision somebody took on purpose, and printing it on every run would turn the whole notice
    // into noise that hides the one line that matters.
    //
    // One warning per contested extension rather than one for the lot, because each names a
    // different extension and that is what a reader of the document wants to key on. What reaches
    // the terminal is unchanged: the blocks were joined by a blank line, and separate lines each
    // carrying a leading one produce the same text.
    //
    // Returned as values and emitted by the caller, so that what a report is worth can be tested
    // without going through the collector that the whole process shares.
    pub fn warnings(&self) -> Vec<warnings::Warning> {
        let mut reported = Vec::new();
        for collision in self.collisions.iter().filter(|x| x.resolved_by == ResolvedBy::AlphabeticalFallback) {
            reported.push(warnings::Warning::new(warnings::EXTENSION_TIEBREAK, warnings::Affects::Counts, &collision.extension,
                    format!("The extension '{}' is claimed by {} and {}. It was given to {} only because that name comes first \
alphabetically, so the files of the rest are counted with the wrong comment and string symbols.\nDeclare it in '{}', or run with '--force-lang {}=<language>'.",
                    collision.extension, collision.winner, collision.losers.join(", "), collision.winner,
                    EXTENSION_PRIORITY_FILE_NAME, collision.extension)));
        }

        for (extension, wanted) in &self.unknown_forced_languages {
            reported.push(warnings::Warning::new(warnings::UNKNOWN_FORCED_LANGUAGE, warnings::Affects::Settings, extension,
                    format!("'--force-lang {extension}={wanted}' names a language that is not available, so the extension was left as it was.")));
        }

        reported
    }
}

// Extensions are matched without regard to case, so the keys are lowercased here, once, and the
// lookup lowercases what it is given. This has to happen before the claimants are counted: with the
// declarations left as they were written, 'cs' and 'CS' would look like two different extensions,
// would never be found to collide, and would each win silently in different files.
pub fn make_extension_language_map(languages: &HashMap<String,Language>, priority: &HashMap<String,Vec<String>>,
        forced: &HashMap<String,String>) -> (HashMap<String, Arc<str>>, ExtensionReport)
{
    let mut names = languages.keys().collect::<Vec<_>>();
    names.sort_unstable();

    let shared_names : HashMap<&str, Arc<str>> = names.iter()
            .map(|name| (name.as_str(), Arc::from(name.as_str())))
            .collect();

    // Normalised once, so that the two places that consult it cannot disagree about the shape of a
    // key. A caller of the library sets this field directly and is under no obligation to lowercase
    // it, and when only one of the two lookups did, the mapping was applied while the run also
    // warned that the extension had been left to the alphabetical tiebreak.
    // The leading dot is stripped here and not only where a command line is parsed, or a caller
    // writing '.rs' would silently match no extension at all: the claimants below are keyed on the
    // bare form.
    let forced : HashMap<String, &str> = forced.iter()
            .map(|(extension, language)| (extension_key(extension), language.as_str()))
            .collect();
    // Searched in the sorted order the names already have, and not through the keys of a map, whose
    // iteration order is arbitrary: two languages whose names differ only in case would otherwise
    // resolve to a different one of the two between runs of the same command.
    //
    // The exact spelling wins before case is folded, because folding it first cannot be undone. With
    // both 'Rust' and 'rust' declared, '--force-lang rs=rust' named one of them and got the other,
    // the one whose capital sorts first, together with its comment symbols and in silence: the user
    // had typed the whole name of a language that exists and there was no way left to select it. The
    // fold stays as the fallback it was meant to be, for when nothing matches letter for letter.
    let language_named = |wanted: &str| names.iter().find(|name| name.as_str() == wanted)
            .or_else(|| names.iter().find(|name| crate::languages::is_the_same_language_name(name, wanted)))
            .map(|x| x.as_str());

    // The leading dot is stripped here as well as in the two other places an extension becomes a
    // key, the forced pairs above and the rules of the priority file. It is the form every editor
    // and every other counter writes, and while only one of the three understood it, a language
    // declaring '.rs' claimed nothing at all and said nothing about it.
    let mut claimants : HashMap<String, Vec<&str>> = HashMap::with_capacity(languages.len() * 2);
    for name in &names {
        for extension in &languages[*name].extensions {
            let claiming = claimants.entry(extension_key(extension)).or_default();
            // A language claiming the same extension twice is not a contest, and it became able to
            // reach one the moment the key stopped keeping '.h' and 'h' apart. Left in, it made a
            // language the rival of itself: the collision fired, the list of losers had every entry
            // equal to the winner and came out empty, and the report read "claimed by Cish and ."
            // The counts were right the whole time, which is what makes it worth dropping in silence
            // rather than announcing: there is nothing here for the reader to go and fix.
            if !claiming.contains(&name.as_str()) {
                claiming.push(name.as_str());
            }
        }
    }

    let mut map : HashMap<String, Arc<str>> = HashMap::with_capacity(claimants.len());
    let mut report = ExtensionReport::default();

    for (extension, claimants) in claimants {
        let forced_winner = forced.get(&extension).and_then(|wanted| language_named(wanted));
        // Exact before folded, for the same reason as the lookup above: a rule of the priority file
        // naming one of two spellings has no other way to say which it meant.
        let priority_winner = priority.get(&extension)
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
            report.collisions.push(ExtensionCollision {
                extension: extension.clone(),
                winner: winner.to_owned(),
                losers: claimants.iter().filter(|name| **name != winner).map(|name| (*name).to_owned()).collect(),
                resolved_by
            });
        }

        map.insert(extension, shared_names[winner].clone());
    }

    // '--force-lang txt=python' is meant to work whether or not anything else claims the extension,
    // so a forced entry that no language claims is added rather than ignored
    for (extension, wanted) in &forced {
        match language_named(wanted) {
            Some(name) => { map.insert(extension.clone(), shared_names[name].clone()); },
            None => report.unknown_forced_languages.push((extension.clone(), (*wanted).to_owned()))
        }
    }

    report.collisions.sort_by(|a, b| a.extension.cmp(&b.extension));
    report.unknown_forced_languages.sort();
    (map, report)
}

// The one spelling of an extension that everything keys on: no leading dot, lowercased the way the
// lookup lowercases what it is handed. Non-ASCII is left alone, since 'to_ascii_lowercase' is what
// the lookup uses and the two must agree on every byte.
pub(crate) fn extension_key(extension: &str) -> String {
    extension.trim_start_matches('.').to_ascii_lowercase()
}

pub fn find_language_of_extension(extension_lang_map: &HashMap<String, Arc<str>>, extension: &str) -> Option<Arc<str>> {
    if let Some(x) = extension_lang_map.get(extension) {
        return Some(x.clone());
    }

    // Every key is already lowercase, so anything that is too, has simply not been found
    if !extension.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }

    // The buffer below is the hot path and covers every extension anybody actually writes. One
    // longer than it comes from somebody's own language file, and is worth the allocation rather
    // than the silent miss it used to be: the file was simply never counted and nothing said so.
    if extension.len() > MAX_EXTENSION_LEN {
        return extension_lang_map.get(&extension.to_ascii_lowercase()).cloned();
    }

    let mut buffer = [0u8; MAX_EXTENSION_LEN];
    let length = extension.len();
    buffer[..length].copy_from_slice(extension.as_bytes());
    buffer[..length].make_ascii_lowercase();
    std::str::from_utf8(&buffer[..length]).ok()
            .and_then(|lowercased| extension_lang_map.get(lowercased))
            .cloned()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages_claiming;

    // Three places turn an extension into a key: a language's own declaration, a '--force-lang'
    // pair, and a rule of the priority file. Only the forced one used to strip a leading dot, so a
    // language declaring '.dot' claimed nothing and a rule written '.m' settled nothing, both in
    // silence. The dotted form is what every editor and every other counter writes.
    #[test]
    fn an_extension_is_keyed_the_same_way_wherever_it_is_declared() {
        let dotted = languages_claiming(&[("Dotty", &[".dot"])]);
        let (map, _) = make_extension_language_map(&dotted, &HashMap::new(), &HashMap::new());
        assert_eq!(Some("Dotty"), map.get("dot").map(|x| x.as_ref()),
                "a language declaring '.dot' claims nothing: {map:?}");

        // and a rule of the priority file reaches the same key
        let (rules, faulty) = crate::language_file::parse_priority(
                "===> contested-extensions\n.m       MATLAB, Objective-C\n");
        assert!(faulty.is_empty());
        assert_eq!(Some(&vec!["MATLAB".to_owned(), "Objective-C".to_owned()]), rules.get("m"));

        let contested = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &[".m"])]);
        let (map, report) = make_extension_language_map(&contested, &rules, &HashMap::new());
        assert_eq!(Some("MATLAB"), map.get("m").map(|x| x.as_ref()));
        assert_eq!(1, report.collisions.len(), "one declared with a dot and one without did not meet");
        assert_eq!(ResolvedBy::PriorityFile, report.collisions[0].resolved_by);
    }

    // The stack buffer is sized for every extension that exists today, and anything longer with a
    // capital in it used to be given up on rather than lowercased, so the files were not counted
    // and nothing said so.
    #[test]
    fn an_extension_longer_than_the_buffer_is_still_matched_case_insensitively() {
        let long = "A".repeat(MAX_EXTENSION_LEN + 6);
        let languages = languages_claiming(&[("Longy", &[long.as_str()])]);
        let (map, _) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!(Some("Longy"), find_language_of_extension(&map, &long.to_lowercase()).as_deref());
        assert_eq!(Some("Longy"), find_language_of_extension(&map, &long).as_deref(),
                "an extension of {} bytes was given up on instead of lowercased", long.len());
        // and one that is genuinely absent is still absent, whatever its length
        assert_eq!(None, find_language_of_extension(&map, &"B".repeat(MAX_EXTENSION_LEN + 6)));
    }

    fn priority(rules: &[(&str, &[&str])]) -> HashMap<String,Vec<String>> {
        rules.iter().map(|(extension, order)| ((*extension).to_owned(),
                order.iter().map(|x| (*x).to_owned()).collect())).collect()
    }

    fn winner_of(map: &HashMap<String, Arc<str>>, extension: &str) -> String {
        find_language_of_extension(map, extension).map(|x| x.to_string()).unwrap_or_default()
    }

    #[test]
    fn an_extension_that_only_one_language_claims_is_never_reported() {
        let languages = languages_claiming(&[("Rust", &["rs"]), ("Go", &["go"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());
    
        assert_eq!("Rust", winner_of(&map, "rs"));
        assert_eq!("Go", winner_of(&map, "go"));
        assert_eq!(ExtensionReport::default(), report);
        assert!(report.warnings().is_empty());
    }

    // The tiebreak is the outcome nobody chose, and the only one that is announced
    #[test]
    fn a_contested_extension_falls_back_to_the_first_name_alphabetically_and_says_so() {
        let languages = languages_claiming(&[("Objective-C", &["m", "mm"]), ("MATLAB", &["m"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!("Objective-C", winner_of(&map, "mm"));
        assert_eq!(vec![ExtensionCollision {
            extension: "m".to_owned(),
            winner: "MATLAB".to_owned(),
            losers: vec!["Objective-C".to_owned()],
            resolved_by: ResolvedBy::AlphabeticalFallback
        }], report.collisions);
        assert_eq!(vec![(warnings::EXTENSION_TIEBREAK, "counts")],
                report.warnings().iter().map(|x| (x.code, x.affects.name())).collect::<Vec<_>>());
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
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!("Cish", winner_of(&map, "h"));
        assert_eq!(1, map.len(), "the three spellings did not fold into one key");
        assert!(report.collisions.is_empty(), "a language was reported as contesting itself: {:?}", report.collisions);
        assert!(report.warnings().is_empty(), "{:?}", report.warnings());

        // and a real contest over the same extension is still announced, so what is gone is the
        // self-collision and not the check
        let contested = languages_claiming(&[("Cish", &[".h"]), ("Bish", &["h"])]);
        let (_, report) = make_extension_language_map(&contested, &HashMap::new(), &HashMap::new());
        assert_eq!(vec!["Cish".to_owned()], report.collisions[0].losers);
    }

    #[test]
    fn the_priority_file_decides_it_and_force_lang_overrules_the_priority_file() {
        let languages = languages_claiming(&[("Objective-C", &["m"]), ("MATLAB", &["m"])]);
    
        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &HashMap::new());
        assert_eq!("Objective-C", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::PriorityFile, report.collisions[0].resolved_by);
        assert_eq!(vec!["MATLAB".to_owned()], report.collisions[0].losers);
    
        let forced = hashmap!("m".to_owned() => "matlab".to_owned());
        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &forced);
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.collisions[0].resolved_by);
    
        // and neither of them is the tiebreak, so neither is announced
        assert!(report.warnings().is_empty());
    }

    // A rule whose every name has been renamed away, removed or misspelled settles nothing, and the
    // tiebreak is what decides. Reporting it as settled left the user believing their rule was in
    // force while the extension quietly went elsewhere, with nothing printed.
    #[test]
    fn a_priority_rule_that_names_no_claimant_falls_through_to_the_tiebreak_and_says_so() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["ObjC"])]), &HashMap::new());
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::AlphabeticalFallback, report.collisions[0].resolved_by);
        let reported = report.warnings();
        assert_eq!(warnings::EXTENSION_TIEBREAK, reported[0].code);
        assert_eq!("m", reported[0].subject);
        assert!(reported[0].message.contains("only because"));
    }

    // A name in the priority file that no longer exists is skipped rather than left to win nothing
    #[test]
    fn the_priority_file_moves_on_to_the_next_name_when_the_first_is_not_there() {
        let languages = languages_claiming(&[("Prolog", &["pl"]), ("Raku", &["pl"])]);
        let (map, _) = make_extension_language_map(&languages, &priority(&[("pl", &["Perl", "Raku", "Prolog"])]), &HashMap::new());
    
        assert_eq!("Raku", winner_of(&map, "pl"));
    }

    #[test]
    fn a_forced_extension_is_taken_even_when_no_language_claims_it() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("txt".to_owned() => "python".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);
    
        assert_eq!("Python", winner_of(&map, "txt"));
        assert_eq!("Python", winner_of(&map, "py"));
        // nothing was contested, so there is nothing to report
        assert!(report.collisions.is_empty());
    }

    // A caller of the library sets the field directly and is under no obligation to lowercase its
    // keys. When only the second of the two lookups normalised, the mapping was applied and the run
    // warned in the same breath that the extension had been left to the alphabetical tiebreak.
    #[test]
    fn a_forced_extension_is_normalised_before_it_is_looked_up() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let forced = hashmap!("M".to_owned() => "MatLab".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);
    
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.collisions[0].resolved_by);
        assert!(report.warnings().is_empty());
    }

    #[test]
    fn a_forced_language_that_is_not_available_is_reported_and_changes_nothing() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("py".to_owned() => "cobol".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);
    
        assert_eq!("Python", winner_of(&map, "py"));
        assert_eq!(vec![("py".to_owned(), "cobol".to_owned())], report.unknown_forced_languages);
        let reported = report.warnings();
        assert_eq!(warnings::UNKNOWN_FORCED_LANGUAGE, reported[0].code);
        // a mapping that did not apply leaves the counts alone, it is the settings that were not honoured
        assert_eq!("settings", reported[0].affects.name());
        assert_eq!("py", reported[0].subject);
        assert!(reported[0].message.contains("not available"));
    }

    // Two spellings of one extension are one extension, and they have to collide as one. Left as
    // they were written they would look like two, would never be found to contest anything, and
    // would each quietly win in the files that happened to be spelled their way.
    #[test]
    fn extensions_are_matched_without_case_and_contest_each_other_across_it() {
        let languages = languages_claiming(&[("Zig", &["ZIG"]), ("Ziggy", &["zig"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());
    
        assert_eq!(1, report.collisions.len());
        assert_eq!("zig", report.collisions[0].extension);
        assert_eq!("Zig", winner_of(&map, "zig"));
        assert_eq!("Zig", winner_of(&map, "ZIG"));
        assert_eq!("Zig", winner_of(&map, "Zig"));
        assert_eq!("", winner_of(&map, "zigg"));
    }
}
