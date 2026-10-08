// The '--explain' face: one file line by line, as text and as JSON. The answers come from the
// library's explain pass; everything here is wording and layout.
use std::path::Path;
use std::process::ExitCode;

use mezura_core::{Bucket, Carried, CountingModel, EngineConfig, ExplainError, FileExplanation, Languages,
        LineClasses, PathPatterns, ScanSkip, Span, SpanKind, TestFileRule};

use crate::config_manager::{Configuration, ExplainedLines, get_command_that_counts};
use crate::json_printer::escape;
use crate::message_printer::wrap_message;
use crate::theme::get_active;

pub fn run_explain(config: &Configuration, languages: Languages) -> ExitCode {
    let [target] = &config.engine.targets[..] else {
        return refuse(&format!("'--explain' answers for exactly one file, and this run names {} \
                targets. Give the file alone: mezura src/main.rs --explain",
                config.engine.targets.len()));
    };
    let path = Path::new(&target.path);
    if !path.is_file() {
        return refuse(&format!("'--explain' answers for one file, and '{}' is not one. Give the \
                file itself: mezura src/main.rs --explain", target.path));
    }

    // Refused, so that asking for one format never quietly hands back another
    if config.view.output == crate::config_manager::OutputFormat::Markdown {
        return refuse("'--explain' answers line by line and has no markdown form. Use '--output \
                text' to read it, or '--output json' for a program to read it.");
    }

    // The document promises one entry per line of the file, and that promise is what a program
    // reading it is written against. Narrowing it would break them for nothing, since a program
    // holding the whole answer takes the lines it wants for free.
    let asked_for = config.view.explain.unwrap_or(ExplainedLines::WHOLE_FILE);
    if asked_for != ExplainedLines::WHOLE_FILE && !config.view.prints_text() {
        return refuse("'--explain' was given lines to show and '--output json' writes an entry for \
                every line of the file, which is what a program reading it expects. Ask for one or \
                the other.");
    }

    match mezura_core::explain_file(path, &read_names_as_a_run_from_here_would(&config.engine), languages) {
        Ok(explanation) => {
            if config.view.prints_text() {
                print_text(&target.path, &explanation, config.view.counting, asked_for);
            } else {
                outln!("{}", build_json_document(&target.path, &explanation, config.view.counting));
            }
            ExitCode::SUCCESS
        },
        Err(ExplainError::UnclaimedFile) => {
            let read_no_first_line = match config.engine.detect_shebangs {
                true => "",
                false => " This run was given '--no-shebang', so a file with no extension is \
                        claimed by nothing."
            };
            refuse(&format!("No language of this run claims '{}', \
                    so there is nothing to explain. If its language was narrowed away, drop \
                    '--languages' or '--exclude-languages'; if the extension is unknown, \
                    '--force-language' hands it to a language.{read_no_first_line}",
                    target.path))
        },
        Err(ExplainError::UnreadableFile(reason)) => refuse(&format!("'{}' could not be read: \
                {reason}", target.path)),
        Err(ExplainError::LanguagesFromAnotherConfig) => refuse("The languages of this run were \
                resolved against different settings, so the answer would be for the wrong \
                selection."),
        // 'ExplainError' is non_exhaustive, so a reason added later stops here rather than in the
        // middle of a run
        Err(other) => refuse(&format!("'{}' could not be explained: {other}", target.path))
    }
}

fn refuse(message: &str) -> ExitCode {
    eprintln!("\n{}\n", get_active().error.paint(&wrap_message(message)));
    ExitCode::FAILURE
}

// The core applies the directory only to a file inside it, so nothing is checked here.
fn read_names_as_a_run_from_here_would(engine: &EngineConfig) -> EngineConfig {
    if engine.test_patterns.read_from.is_some() {
        return engine.clone();
    }
    let working_dir = std::env::current_dir().ok()
            .map(|dir| crate::paths::normalise_separators(&dir.to_string_lossy()).into_owned());

    EngineConfig { test_patterns: PathPatterns { patterns: engine.test_patterns.patterns.clone(), read_from: working_dir },
            ..engine.clone() }
}

fn print_text(path: &str, explanation: &FileExplanation, model: CountingModel, asked_for: ExplainedLines) {
    let theme = get_active();
    outln!("{}", theme.explain_heading.paint(&format!("{path} as {}, counted by {}",
            explanation.language, model.name())));
    if let Some((literal, line)) = &explanation.identified_by {
        outln!("{}", theme.note.paint(&format!("Its extension is contested, and line {line} \
                carrying '{literal}' is what identified it.")));
    }
    if let Some(skip) = explanation.left_out_of_a_scan {
        let reason = match skip {
            ScanSkip::Minified => "minified",
            ScanSkip::Generated => "generated",
            ScanSkip::NotCode => "not code"
        };
        outln!("{}", theme.note.paint(&format!("A directory scan leaves this file out as \
                {reason}. It is counted here because you named it. '--{}' stops leaving it out for that reason.",
                get_command_that_counts(skip))));
    }
    if let Some(sentence) = explanation.test_file_rule.as_ref().and_then(|rule| describe_test_file_rule(rule, &explanation.language)) {
        outln!("{}", theme.note.paint(&sentence));
    }
    outln!();
    if explanation.lines.is_empty() {
        outln!("{}", theme.note.paint("The file has no lines."));
        return;
    }

    let whole_file = asked_for.is_the_whole_file(explanation.lines.len());
    if !whole_file {
        outln!("{}", theme.note.paint(&format!("Showing lines {} to {} of {}.", asked_for.first,
                asked_for.last.min(explanation.lines.len()), explanation.lines.len())));
        outln!();
    }

    let width = explanation.lines.len().to_string().len();
    let mut printed = 0;
    for (at, (source, line)) in explanation.contents.lines().zip(&explanation.lines).enumerate() {
        if !asked_for.holds(at + 1) {
            continue;
        }
        if printed > 0 {
            outln!();
        }
        printed += 1;
        let bucket = model.fold(line.class);
        let bucket_style = match bucket {
            Bucket::Code => &theme.explain_code,
            Bucket::Comments => &theme.explain_comments,
            Bucket::Third => &theme.explain_extra
        };
        outln!("{:>width$}  {}", at + 1, paint_by_spans(source, &line.spans));
        let mut verdict = format!("{}  {}", bucket_style.paint(model.get_bucket_name(bucket)),
                theme.explain_detail.paint(line.class.name()));
        let mut notes = line.in_test.then(|| "test code".to_owned()).into_iter().collect::<Vec<_>>();
        notes.extend(line.read_as.as_ref().map(|name| format!("read as {name}")));
        notes.extend(describe_carried(&line.carried));
        if !notes.is_empty() {
            verdict = format!("{verdict}  {}", theme.note.paint(&format!("({})", notes.join("; "))));
        }
        outln!("{:>width$}  {verdict}", "");
    }

    // Two lines when a range was asked for, and the file's own is the second of them: a range total
    // alone says nothing about the count somebody opened '--explain' to check, and the file's alone
    // does not answer how much of the range is comment.
    outln!();
    let tests_in_the_file = count_test_lines(explanation, ExplainedLines::WHOLE_FILE);
    if whole_file {
        print_totals(&explanation.classes, explanation.lines.len(), tests_in_the_file, model, "");
    } else {
        print_totals(&collect_classes_of(explanation, asked_for), printed, count_test_lines(explanation, asked_for),
                model, " shown");
        print_totals(&explanation.classes, explanation.lines.len(), tests_in_the_file, model, " in the file");
    }
}

// The document linejudge reads: 'format', 'lines', 'buckets' and one 'per_line' entry per physical
// line are the contract, everything else is mezura's own and a reader is free to skip it.
fn build_json_document(path: &str, explanation: &FileExplanation, model: CountingModel) -> String {
    let (code, comments, third) = fold_totals(&explanation.classes, explanation.lines.len(), model);
    let mut document = String::with_capacity(120 + 70 * explanation.lines.len());
    document.push_str(&format!("{{\"format\":1,\"counter\":\"mezura\",\"file\":\"{}\",",
            escape(path)));
    document.push_str(&format!("\"language\":\"{}\",\"counting\":\"{}\",\"lines\":{},",
            escape(&explanation.language), model.name(), explanation.lines.len()));
    if let Some((literal, line)) = &explanation.identified_by {
        document.push_str(&format!("\"identified_by\":{{\"line\":{line},\"evidence\":\"{}\"}},",
                escape(literal)));
    }
    if let Some(skip) = explanation.left_out_of_a_scan {
        document.push_str(&format!("\"left_out_of_a_scan\":\"{}\",", skip.name()));
    }
    if let Some(entry) = explanation.test_file_rule.as_ref().and_then(build_test_file_entry) {
        document.push_str(&format!("\"test_file\":{entry},"));
    }
    document.push_str(&format!("\"buckets\":{{\"code\":{code},\"comments\":{comments},\"{}\":{third}}},",
            model.get_third_quantity_name()));
    document.push_str("\"per_line\":[");
    for (at, line) in explanation.lines.iter().enumerate() {
        if at > 0 {
            document.push(',');
        }
        let bucket = model.get_bucket_name(model.fold(line.class));
        document.push_str(&format!("{{\"line\":{},\"bucket\":\"{bucket}\",\"class\":\"{}\"",
                at + 1, line.class.name()));
        if line.in_test {
            document.push_str(",\"in_test\":true");
        }
        if let Some(name) = &line.read_as {
            document.push_str(&format!(",\"read_as\":\"{}\"", escape(name)));
        }
        if let Some(note) = describe_carried(&line.carried) {
            document.push_str(&format!(",\"carried\":\"{}\"", escape(&note)));
        }
        if !line.spans.is_empty() {
            document.push_str(&format!(",\"spans\":[{}]", line.spans.iter()
                    .map(|span| format!("[{},{},\"{}\"]", span.from, span.to, span.kind.name()))
                    .collect::<Vec<_>>().join(",")));
        }
        document.push('}');
    }
    document.push_str("],");
    document.push_str(&format!("\"classes\":{{{}}}}}",
            explanation.classes.to_array().iter().zip(mezura_core::LineClasses::NAMES)
                    .map(|(count, name)| format!("\"{name}\":{count}"))
                    .collect::<Vec<_>>().join(",")));
    document
}

fn print_totals(classes: &LineClasses, lines: usize, tests: usize, model: CountingModel, of_what: &str) {
    let theme = get_active();
    let (code, comments, third) = fold_totals(classes, lines, model);
    let label = format!("{}{of_what}", if lines == 1 {"line"} else {"lines"});
    outln!("{} {}: {} {}, {} {}, {} {}",
            theme.lines_number.paint(&lines.to_string()), theme.lines_label.paint(&label),
            theme.code_number.paint(&code.to_string()), theme.explain_code.paint("code"),
            theme.comments_number.paint(&comments.to_string()), theme.explain_comments.paint("comments"),
            theme.extra_number.paint(&third.to_string()), theme.explain_extra.paint(model.get_third_quantity_name()));
    if let Some(line) = format_test_count(tests) {
        outln!("{line}");
    }
}

fn format_test_count(tests: usize) -> Option<String> {
    let theme = get_active();
    (tests > 0).then(|| format!("{} {}", theme.tests_lines.paint(&tests.to_string()),
            theme.tests_name.paint(if tests == 1 {"of them is test code"} else {"of them are test code"})))
}

fn count_test_lines(explanation: &FileExplanation, asked_for: ExplainedLines) -> usize {
    explanation.lines.iter().enumerate().filter(|(at, line)| line.in_test && asked_for.holds(at + 1)).count()
}

// The nine counts of the lines that were printed, built the way the parser builds the file's own,
// so that a tenth class arrives here without anybody remembering to come and add it.
fn collect_classes_of(explanation: &FileExplanation, asked_for: ExplainedLines) -> LineClasses {
    let mut classes = LineClasses::default();
    for (at, line) in explanation.lines.iter().enumerate() {
        if asked_for.holds(at + 1) {
            classes.bump(line.class);
        }
    }

    classes
}

fn fold_totals(classes: &LineClasses, lines: usize, model: CountingModel) -> (usize, usize, usize) {
    let code = model.calculate_code_lines(classes);
    let comments = model.calculate_comment_lines(classes);
    (code, comments, lines - code - comments)
}

// A span that would cut a character in half is left unpainted rather than panicking over a
// diagnostic.
fn paint_by_spans(source: &str, spans: &[Span]) -> String {
    let theme = get_active();
    let mut painted = String::with_capacity(source.len() + 16 * spans.len());
    let mut at = 0;
    for span in spans {
        let (from, to) = (span.from.min(source.len()), span.to.min(source.len()));
        if from > at && source.is_char_boundary(at) && source.is_char_boundary(from) {
            painted.push_str(&source[at..from]);
        }
        if !source.is_char_boundary(from) || !source.is_char_boundary(to) {
            continue;
        }
        let piece = &source[from..to];
        match span.kind {
            SpanKind::String => painted.push_str(&theme.explain_string.paint(piece).to_string()),
            SpanKind::Comment => painted.push_str(&theme.explain_comment.paint(piece).to_string()),
            SpanKind::Code => painted.push_str(piece)
        }
        at = to;
    }
    if at <= source.len() && source.is_char_boundary(at) {
        painted.push_str(&source[at..]);
    }
    painted
}

fn describe_carried(carried: &Carried) -> Option<String> {
    match carried {
        Carried::Nothing => None,
        Carried::Comment { opener, since_line, ends_on_this_line: true, .. } => Some(format!(
                "the comment opened by {opener} on line {since_line} ends on this line")),
        Carried::Comment { opener, depth, since_line, .. } if *depth > 1 => Some(format!(
                "in a comment opened by {opener} on line {since_line}, {depth} deep")),
        Carried::Comment { opener, since_line, .. } => Some(format!(
                "in a comment opened by {opener} on line {since_line}")),
        Carried::Str { opener, since_line, ends_on_this_line: true } => Some(format!(
                "the string opened by {opener} on line {since_line} ends on this line")),
        Carried::Str { opener, since_line, .. } => Some(format!(
                "in a string opened by {opener} on line {since_line}")),
        Carried::CommentContinuation { since_line } => Some(format!(
                "a continuation of the comment on line {since_line}")),
    }
}

// The rules are non_exhaustive, so a rule the library adds later is silently left out here and in
// the document until it is given its words
fn describe_test_file_rule(rule: &TestFileRule, language: &str) -> Option<String> {
    match rule {
        TestFileRule::TestDirectory { directory, build_files } => Some(format!("The whole file is test code: it \
                is inside '{directory}', the test folder of {}.",
                build_files.iter().map(|file| format!("'{file}'")).collect::<Vec<_>>().join(" and "))),
        TestFileRule::FileName(shape) => Some(format!("The whole file is test code: {language} names its test \
                files '{shape}'.")),
        TestFileRule::Declared { pattern, matched_folder: Some(folder) } => Some(format!("The whole file is test \
                code: the '--tests' pattern '{pattern}' matches the folder '{folder}'.")),
        TestFileRule::Declared { pattern, matched_folder: None } => Some(format!("The whole file is test code: the \
                '--tests' pattern '{pattern}' matches it.")),
        TestFileRule::DeclaredNotTests { pattern, matched_folder } => Some(format!("The '--tests' pattern \
                '{pattern}'{} says this file is not test code, even if its folder or its name would make it so. \
                Lines inside a marker that opens test code still count as tests.", matched_folder.as_ref()
                        .map(|folder| format!(" matches the folder '{folder}' and")).unwrap_or_default())),
        TestFileRule::DeclaredModule { declared_by } => match declared_by.as_slice() {
            [] => None,
            [marked] => Some(format!("The whole file is test code: '{marked}' declares it as a module under a test marker.")),
            [first, between @ .., marked] => Some(format!("The whole file is test code: '{first}' declares it as a module, {}and \
                    '{marked}' declares that one under a test marker.", between.iter()
                            .map(|file| format!("'{file}' declares that one, ")).collect::<String>()))
        },
        TestFileRule::FileMarker { marker, line } => Some(format!("The whole file is test code: '{marker}' on line {line} \
                is one of the names a test runner collects tests by, and a file of {language} holding one is test code whole.")),
        _ => None
    }
}

fn build_test_file_entry(rule: &TestFileRule) -> Option<String> {
    let matched = |folder: &Option<String>| folder.as_ref()
            .map(|folder| format!(",\"matched_folder\":\"{}\"", escape(folder))).unwrap_or_default();
    match rule {
        TestFileRule::TestDirectory { directory, build_files } => Some(format!(
                "{{\"rule\":\"test_directory\",\"directory\":\"{}\",\"build_files\":[{}]}}", escape(directory),
                build_files.iter().map(|file| format!("\"{}\"", escape(file))).collect::<Vec<_>>().join(","))),
        TestFileRule::FileName(shape) => Some(format!("{{\"rule\":\"file_name\",\"shape\":\"{}\"}}",
                escape(&shape.to_string()))),
        TestFileRule::Declared { pattern, matched_folder } => Some(format!(
                "{{\"rule\":\"declared\",\"pattern\":\"{}\"{}}}", escape(pattern), matched(matched_folder))),
        TestFileRule::DeclaredNotTests { pattern, matched_folder } => Some(format!(
                "{{\"rule\":\"declared_not_tests\",\"pattern\":\"{}\"{}}}", escape(pattern), matched(matched_folder))),
        TestFileRule::DeclaredModule { declared_by } => Some(format!("{{\"rule\":\"declared_module\",\"declared_by\":[{}]}}",
                declared_by.iter().map(|file| format!("\"{}\"", escape(file))).collect::<Vec<_>>().join(","))),
        TestFileRule::FileMarker { marker, line } => Some(format!("{{\"rule\":\"file_marker\",\"marker\":\"{}\",\"line\":{line}}}",
                escape(marker))),
        _ => None
    }
}

#[cfg(test)]
mod tests {
    use mezura_core::{ExplainedLine, LineClass, TestFileName};

    use super::*;

    // The working directory of a test of this crate is the package root, so 'src/main.rs' is inside it
    #[test]
    fn a_name_typed_beside_explain_sees_what_a_run_from_the_working_dir_would_see() {
        let mut engine = EngineConfig::new(["src/main.rs"]);
        engine.test_patterns = PathPatterns::of(["mezura/src/"]);
        let anchored = read_names_as_a_run_from_here_would(&engine);
        let working_dir = crate::paths::normalise_separators(&std::env::current_dir().unwrap().to_string_lossy()).into_owned();
        assert_eq!(Some(working_dir), anchored.test_patterns.read_from);

        let of_a_project = EngineConfig { test_patterns: PathPatterns { patterns: vec!["spec/".to_owned()],
                read_from: Some("D:/proj".to_owned()) }, ..engine.clone() };
        assert_eq!(of_a_project, read_names_as_a_run_from_here_would(&of_a_project));

        let languages_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../mezura-core/data/languages/");
        let parsed = mezura_core::language_file::parse_languages_in_dir(languages_dir).unwrap().0;
        let explain = |engine: &EngineConfig| {
            let languages = Languages::resolve(engine, parsed.clone(), &Default::default()).0;
            mezura_core::explain_file(Path::new("src/main.rs"), engine, languages).unwrap()
        };
        assert!(explain(&anchored).lines.iter().all(|line| line.in_test), "the pattern did not reach the file");
        assert!(explain(&engine).lines.iter().any(|line| !line.in_test), "the name saw a folder above the file's own");
    }

    // Color is turned off because the manual comparison protocol exports CLICOLOR_FORCE and a test
    // binary inherits it, which would leave escape codes in the compared text.
    #[test]
    fn the_painted_line_keeps_every_byte_of_the_source() {
        colored::control::set_override(false);
        let line = " два\"; // ok";
        let spans = [Span { from: 1, to: 8, kind: SpanKind::String },
                Span { from: 8, to: 9, kind: SpanKind::Code },
                Span { from: 10, to: 15, kind: SpanKind::Comment }];
        assert_eq!(line, paint_by_spans(line, &spans));
        assert_eq!(line, paint_by_spans(line, &[]));
        assert_eq!(line, paint_by_spans(line, &[Span { from: 2, to: 40, kind: SpanKind::String }]));
    }

    #[test]
    fn a_carried_answer_reads_as_a_sentence() {
        assert_eq!(None, describe_carried(&Carried::Nothing));
        assert_eq!(Some("in a comment opened by /* on line 23".to_owned()),
                describe_carried(&Carried::Comment { opener: "/*".to_owned(), depth: 1, since_line: 23,
                        ends_on_this_line: false }));
        assert_eq!(Some("in a comment opened by --[[ on line 4, 3 deep".to_owned()),
                describe_carried(&Carried::Comment { opener: "--[[".to_owned(), depth: 3, since_line: 4,
                        ends_on_this_line: false }));
        assert_eq!(Some("the comment opened by /* on line 12 ends on this line".to_owned()),
                describe_carried(&Carried::Comment { opener: "/*".to_owned(), depth: 1, since_line: 12,
                        ends_on_this_line: true }));
        assert_eq!(Some("in a string opened by \" on line 7".to_owned()),
                describe_carried(&Carried::Str { opener: "\"".to_owned(), since_line: 7,
                        ends_on_this_line: false }));
        assert_eq!(Some("the string opened by \"\"\" on line 2 ends on this line".to_owned()),
                describe_carried(&Carried::Str { opener: "\"\"\"".to_owned(), since_line: 2,
                        ends_on_this_line: true }));
        assert_eq!(Some("a continuation of the comment on line 2".to_owned()),
                describe_carried(&Carried::CommentContinuation { since_line: 2 }));
    }

    #[test]
    fn the_rule_of_a_whole_test_file_and_the_count_of_its_test_lines_read_as_sentences() {
        colored::control::set_override(false);
        let owned = |texts: &[&str]| texts.iter().map(|text| text.to_string()).collect::<Vec<_>>();
        assert_eq!(Some("The whole file is test code: it is inside 'D:/proj/tests', the test folder of \
                'D:/proj/DESCRIPTION' and 'D:/proj/NAMESPACE'.".to_owned()),
                describe_test_file_rule(&TestFileRule::TestDirectory { directory: "D:/proj/tests".to_owned(),
                        build_files: owned(&["D:/proj/DESCRIPTION", "D:/proj/NAMESPACE"]) }, "R"));
        assert_eq!(Some("The whole file is test code: Go names its test files '*_test.go'.".to_owned()),
                describe_test_file_rule(&TestFileRule::FileName(TestFileName::EndsWith("_test.go".to_owned())), "Go"));
        assert_eq!(Some("The whole file is test code: the '--tests' pattern 'spec/' matches the folder 'D:/dev/proj/spec'.".to_owned()),
                describe_test_file_rule(&TestFileRule::Declared { pattern: "spec/".to_owned(),
                        matched_folder: Some("D:/dev/proj/spec".to_owned()) }, "Rust"));
        assert_eq!(Some("The whole file is test code: the '--tests' pattern '*.spec.rs' matches it.".to_owned()),
                describe_test_file_rule(&TestFileRule::Declared { pattern: "*.spec.rs".to_owned(), matched_folder: None }, "Rust"));
        assert_eq!(Some("The '--tests' pattern '!spec/fixtures/' matches the folder 'D:/dev/proj/spec/fixtures' and says \
                this file is not test code, even if its folder or its name would make it so. Lines inside a marker that \
                opens test code still count as tests.".to_owned()),
                describe_test_file_rule(&TestFileRule::DeclaredNotTests { pattern: "!spec/fixtures/".to_owned(),
                        matched_folder: Some("D:/dev/proj/spec/fixtures".to_owned()) }, "Rust"));
        assert_eq!(Some("The '--tests' pattern '!a.rs' says this file is not test code, even if its folder or its \
                name would make it so. Lines inside a marker that opens test code still count as tests.".to_owned()),
                describe_test_file_rule(&TestFileRule::DeclaredNotTests { pattern: "!a.rs".to_owned(), matched_folder: None }, "Rust"));
        let declared_by = |files: &[&str]| TestFileRule::DeclaredModule { declared_by: owned(files) };
        assert_eq!(Some("The whole file is test code: 'D:/proj/src/lib.rs' declares it as a module under a test marker.".to_owned()),
                describe_test_file_rule(&declared_by(&["D:/proj/src/lib.rs"]), "Rust"));
        assert_eq!(Some("The whole file is test code: 'D:/proj/src/tests.rs' declares it as a module, and 'D:/proj/src/lib.rs' \
                declares that one under a test marker.".to_owned()),
                describe_test_file_rule(&declared_by(&["D:/proj/src/tests.rs", "D:/proj/src/lib.rs"]), "Rust"));
        assert_eq!(Some("The whole file is test code: 'a.rs' declares it as a module, 'b.rs' declares that one, 'c.rs' declares \
                that one, and 'd.rs' declares that one under a test marker.".to_owned()),
                describe_test_file_rule(&declared_by(&["a.rs", "b.rs", "c.rs", "d.rs"]), "Rust"));
        assert_eq!(None, describe_test_file_rule(&declared_by(&[]), "Rust"));
        assert_eq!(Some("The whole file is test code: '[Fact' on line 7 is one of the names a test runner collects tests by, \
                and a file of C# holding one is test code whole.".to_owned()),
                describe_test_file_rule(&TestFileRule::FileMarker { marker: "[Fact".to_owned(), line: 7 }, "C#"));

        assert_eq!(None, format_test_count(0));
        assert_eq!(Some("1 of them is test code".to_owned()), format_test_count(1));
        assert_eq!(Some("5 of them are test code".to_owned()), format_test_count(5));
    }

    #[test]
    fn a_test_line_carries_its_mark_and_the_rule_of_the_whole_file_is_a_key_of_the_document() {
        let line = |in_test| ExplainedLine { class: LineClass::WordsInCode, read_as: None, carried: Carried::Nothing,
                spans: Vec::new(), in_test };
        let mut classes = LineClasses::default();
        classes.bump(LineClass::WordsInCode);
        classes.bump(LineClass::WordsInCode);
        let mut explanation = FileExplanation { language: "Rust".to_owned(), identified_by: None, left_out_of_a_scan: None,
                test_file_rule: Some(TestFileRule::DeclaredNotTests { pattern: "!spec/fixtures/".to_owned(),
                        matched_folder: Some("D:/dev/proj/spec/fixtures".to_owned()) }),
                contents: "fn f() {}\n#[cfg(test)] fn t() {}\n".to_owned(), lines: vec![line(false), line(true)], classes };

        let document = build_json_document("D:/dev/proj/spec/fixtures/a.rs", &explanation, CountingModel::Content);
        assert!(document.contains(r#""test_file":{"rule":"declared_not_tests","pattern":"!spec/fixtures/","matched_folder":"D:/dev/proj/spec/fixtures"},"buckets""#),
                "{document}");
        assert!(document.contains(r#"{"line":1,"bucket":"code","class":"words_in_code"}"#), "{document}");
        assert!(document.contains(r#"{"line":2,"bucket":"code","class":"words_in_code","in_test":true}"#), "{document}");

        explanation.test_file_rule = None;
        assert!(!build_json_document("a.rs", &explanation, CountingModel::Content).contains("test_file"));
        assert_eq!(Some(r#"{"rule":"test_directory","directory":"D:/proj/tests","build_files":["D:/proj/Cargo.toml"]}"#.to_owned()),
                build_test_file_entry(&TestFileRule::TestDirectory { directory: "D:/proj/tests".to_owned(),
                        build_files: vec!["D:/proj/Cargo.toml".to_owned()] }));
        assert_eq!(Some(r#"{"rule":"file_name","shape":"*.t"}"#.to_owned()),
                build_test_file_entry(&TestFileRule::FileName(TestFileName::EndsWith(".t".to_owned()))));
        assert_eq!(Some(r#"{"rule":"declared","pattern":"*.spec.rs"}"#.to_owned()),
                build_test_file_entry(&TestFileRule::Declared { pattern: "*.spec.rs".to_owned(), matched_folder: None }));
        assert_eq!(Some(r#"{"rule":"declared_module","declared_by":["D:/proj/src/tests.rs","D:/proj/src/lib.rs"]}"#.to_owned()),
                build_test_file_entry(&TestFileRule::DeclaredModule { declared_by: vec!["D:/proj/src/tests.rs".to_owned(),
                        "D:/proj/src/lib.rs".to_owned()] }));
        assert_eq!(Some(r#"{"rule":"file_marker","marker":"@Test","line":12}"#.to_owned()),
                build_test_file_entry(&TestFileRule::FileMarker { marker: "@Test".to_owned(), line: 12 }));
    }
}
