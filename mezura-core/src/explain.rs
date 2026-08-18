// One file, line by line: the class the walk gave each line, what earlier lines had left open when
// it began, and which language's rules read it. The answers come out of the same walk that counts,
// so they cannot disagree with the totals.
use std::path::Path;

use crate::{EngineConfig, Language, LineClass, LineClasses};
use crate::domain::CommentPair;
use crate::engine::file_parser::{CarriedRecord, NestedLanguageLookup, explain_parsed_file};
use crate::languages::Languages;

pub struct FileExplanation {
    pub language: String,
    pub contents: String,
    pub lines: Vec<ExplainedLine>,
    pub classes: LineClasses,
}

pub struct ExplainedLine {
    pub class: LineClass,
    // Some where a nested language's rules read the line; None means the file's own language did
    pub read_as: Option<String>,
    pub carried: Carried,
}

// What was open when the line began. For a comment of a leveled pair the opener is spelled with
// its level, '--[==[', and the depth is 1; a nesting pair reports its real depth.
#[derive(Debug, PartialEq)]
pub enum Carried {
    Nothing,
    Comment { opener: String, depth: u32, since_line: usize },
    Str { opener: String, since_line: usize },
    CommentContinuation { since_line: usize },
}

#[derive(Debug, PartialEq)]
pub enum ExplainError {
    // Refused for the reason 'run' refuses the same pair: the answer would look normal and be for
    // a different set of languages than the settings describe
    LanguagesFromAnotherConfig,
    UnclaimedFile,
    UnreadableFile(String),
}

pub fn explain_file(path: &Path, config: &EngineConfig, languages: Languages)
    -> Result<FileExplanation, ExplainError>
{
    if !languages.describe_the_same_selection_as(config) {
        return Err(ExplainError::LanguagesFromAnotherConfig);
    }
    let (by_name, lookup, nested_definitions) = languages.into_parts();
    let Some(lang_name) = lookup.of_path(path) else {
        return Err(ExplainError::UnclaimedFile);
    };
    let nested_lookup = NestedLanguageLookup {
        languages: &by_name,
        extension_to_name: &nested_definitions.extension_to_name,
        set_aside: &nested_definitions.set_aside,
    };
    let (contents, report, log) = explain_parsed_file(path, &lang_name, &nested_lookup, config)
            .map_err(ExplainError::UnreadableFile)?;

    let language = lang_name.to_string();
    let lines = log.records().iter().map(|record| {
        let read_by = log.get_language_name_of(record);
        ExplainedLine {
            class: record.class,
            read_as: (read_by != language).then(|| read_by.to_owned()),
            carried: spell_out_carried(record.carried, nested_lookup.find_by_name(read_by)),
        }
    }).collect::<Vec<_>>();

    let whole = report.into_whole();
    debug_assert_eq!(whole.lines, lines.len(),
            "a file of {} lines got {} per-line records", whole.lines, lines.len());
    Ok(FileExplanation { language, contents, lines, classes: whole.classes })
}

// The record holds symbol numbers; what a reader gets is the symbol as the file spells it. The
// language is the one that read the line, and a record's language always resolves, so the fallback
// arm is never the answer.
fn spell_out_carried(carried: CarriedRecord, language: Option<&Language>) -> Carried {
    match carried {
        CarriedRecord::Nothing => Carried::Nothing,
        CarriedRecord::Continuation { since_line } => Carried::CommentContinuation { since_line },
        CarriedRecord::Str { symbol, since_line } => Carried::Str {
            opener: language.map(|x| x.get_string_pair_of(symbol).0.to_owned()).unwrap_or_default(),
            since_line
        },
        CarriedRecord::Comment { symbol, depth, since_line } => {
            let (opener, depth) = language.map(|x| spell_comment_opener(x, symbol, depth))
                    .unwrap_or((String::new(), depth));
            Carried::Comment { opener, depth, since_line }
        }
    }
}

fn spell_comment_opener(language: &Language, symbol: u8, depth: u32) -> (String, u32) {
    match language.comment_pairs().nth(symbol as usize) {
        Some(CommentPair::Plain { start, .. }) => (start.to_owned(), depth),
        Some(CommentPair::Nesting { start, .. }) => (start.to_owned(), depth),
        // The walk carries the level in the depth slot, and the filler is the '=' the scan plan
        // counts, so the opener comes back exactly as written
        Some(CommentPair::Leveled(pair)) => (format!("{}{}{}", pair.start_prefix,
                "=".repeat(depth as usize), pair.start_suffix as char), 1),
        None => (String::new(), depth),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::{CountingModel, EngineConfig};
    use crate::test_paths;

    use super::*;

    fn resolved_languages(config: &EngineConfig) -> Languages {
        let languages = crate::language_file::parse_languages_in_dir(test_paths::LANGUAGES_DIR).unwrap().0;
        Languages::resolve(config, languages, &Default::default()).0
    }

    fn explain_in_own_dir(test_name: &str, file_name: &str, contents: &str) -> FileExplanation {
        let root = std::env::temp_dir().join(test_name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join(file_name);
        fs::write(&path, contents).unwrap();

        let config = EngineConfig::default();
        let explained = explain_file(&path, &config, resolved_languages(&config)).unwrap();
        fs::remove_dir_all(&root).unwrap();
        explained
    }

    #[test]
    fn a_carried_comment_and_string_name_their_opener_and_its_line() {
        let explained = explain_in_own_dir("mezura-explain-carried", "a.rs",
                "fn main() {\n/* first\n\nlast */\nlet s = \"one\n два\";\n}\n");

        assert_eq!("Rust", explained.language);
        assert_eq!(7, explained.lines.len());
        assert_eq!(explained.lines.len(), explained.classes.calculate_lines());

        let carried = explained.lines.iter().map(|line| &line.carried).collect::<Vec<_>>();
        assert_eq!(&Carried::Nothing, carried[0]);
        assert_eq!(&Carried::Nothing, carried[1]);
        assert_eq!(&Carried::Comment { opener: "/*".to_owned(), depth: 1, since_line: 2 }, carried[2]);
        assert_eq!(&Carried::Comment { opener: "/*".to_owned(), depth: 1, since_line: 2 }, carried[3]);
        assert_eq!(&Carried::Nothing, carried[4]);
        assert_eq!(&Carried::Str { opener: "\"".to_owned(), since_line: 5 }, carried[5]);
        assert_eq!(&Carried::Nothing, carried[6]);

        assert_eq!(LineClass::BlankInComment, explained.lines[2].class);
        assert_eq!(LineClass::StringContent, explained.lines[5].class);
        // nothing in this file is read by another language
        assert!(explained.lines.iter().all(|line| line.read_as.is_none()));
    }

    #[test]
    fn a_leveled_opener_is_spelled_with_its_level() {
        let explained = explain_in_own_dir("mezura-explain-leveled", "a.lua",
                "x = 1\n--[==[ words\nmore words\n]==]\n");

        assert_eq!("Lua", explained.language);
        assert_eq!(&Carried::Comment { opener: "--[==[".to_owned(), depth: 1, since_line: 2 },
                &explained.lines[2].carried);
    }

    #[test]
    fn every_line_of_a_container_names_the_language_that_read_it() {
        let explained = explain_in_own_dir("mezura-explain-container", "a.html",
                "<p>hello</p>\n<style>\n/* a css comment */\nh1 { color: red; }\n</style>\n");

        let read_as = explained.lines.iter()
                .map(|line| line.read_as.as_deref()).collect::<Vec<_>>();
        // the tag lines stay with the shell, the section's lines say who read them
        assert_eq!(vec![None, None, Some("CSS"), Some("CSS"), None], read_as);
        assert_eq!(LineClass::WordsInComment, explained.lines[2].class);
        // and the carried answers use the section language's own symbols
        assert_eq!(5, explained.classes.calculate_lines());
    }

    #[test]
    fn the_per_line_buckets_add_up_to_the_folded_columns() {
        let explained = explain_in_own_dir("mezura-explain-buckets", "a.rs",
                "fn main() {\n// words\n\nlet x = 1; // words beside code\n}\n");

        for model in [CountingModel::Content, CountingModel::Region] {
            for bucket in [crate::Bucket::Code, crate::Bucket::Comments, crate::Bucket::Third] {
                let per_line = explained.lines.iter()
                        .filter(|line| model.fold(line.class) == bucket).count();
                let folded = match bucket {
                    crate::Bucket::Code => model.calculate_code_lines(&explained.classes),
                    crate::Bucket::Comments => model.calculate_comment_lines(&explained.classes),
                    crate::Bucket::Third => explained.classes.calculate_lines()
                            - model.calculate_code_lines(&explained.classes)
                            - model.calculate_comment_lines(&explained.classes)
                };
                assert_eq!(folded, per_line, "{model:?} {bucket:?}");
            }
        }
    }

    #[test]
    fn a_file_no_language_claims_and_a_missing_file_are_refused_with_their_own_answers() {
        let root = std::env::temp_dir().join("mezura-explain-refusals");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let unclaimed = root.join("a.unclaimed-extension");
        fs::write(&unclaimed, "text\n").unwrap();

        let config = EngineConfig::default();
        assert_eq!(Err(ExplainError::UnclaimedFile),
                explain_file(&unclaimed, &config, resolved_languages(&config))
                        .map(|_| ()));
        assert!(matches!(
                explain_file(&root.join("missing.rs"), &config, resolved_languages(&config)).map(|_| ()),
                Err(ExplainError::UnreadableFile(_))));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn languages_resolved_against_other_settings_are_refused() {
        let narrowed = EngineConfig {
            languages_of_interest: vec!["Rust".to_owned()],
            ..EngineConfig::default()
        };
        let languages = resolved_languages(&narrowed);
        assert_eq!(Err(ExplainError::LanguagesFromAnotherConfig),
                explain_file(&PathBuf::from("a.rs"), &EngineConfig::default(), languages).map(|_| ()));
    }

    // Keywords cannot move a class, so hiding them changes nothing here; asserted because the
    // explain pass forces them off for speed whatever the configuration says
    #[test]
    fn the_answer_is_the_same_with_keywords_on_and_off() {
        let root = std::env::temp_dir().join("mezura-explain-keywords");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("a.rs");
        fs::write(&path, "struct A;\n// a struct\nfn f() {}\n").unwrap();

        let with = EngineConfig { count_keywords: true, ..EngineConfig::default() };
        let without = EngineConfig { count_keywords: false, ..EngineConfig::default() };
        let explained_with = explain_file(&path, &with, resolved_languages(&with)).unwrap();
        let explained_without = explain_file(&path, &without, resolved_languages(&without)).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(explained_with.classes, explained_without.classes);
        assert_eq!(explained_with.lines.len(), explained_without.lines.len());
    }
}
