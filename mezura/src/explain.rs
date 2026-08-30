// The '--explain' face: one file line by line, as text and as JSON. The answers come from the
// library's explain pass; everything here is wording and layout.
use std::path::Path;
use std::process::ExitCode;

use mezura_core::{Bucket, Carried, CountingModel, ExplainError, FileExplanation, Languages,
        LineClasses, Span, SpanKind};

use crate::config_manager::{Configuration, ExplainedLines};
use crate::json_printer::escape;
use crate::message_printer::wrap_message;
use crate::theme::get_active;

pub fn run_explain(config: &Configuration, languages: Languages) -> ExitCode {
    let [target] = &config.engine.targets[..] else {
        return refuse(&format!("'--explain' answers for exactly one file, and this run names {} \
targets. Give the file alone: mezura src/main.rs --explain", config.engine.targets.len()));
    };
    let path = Path::new(&target.path);
    if !path.is_file() {
        return refuse(&format!("'--explain' answers for one file, and '{}' is not one. Give the \
file itself: mezura src/main.rs --explain", target.path));
    }

    // The document promises one entry per line of the file, and that promise is what a program
    // reading it is written against. Narrowing it would break them for nothing, since a program
    // holding the whole answer takes the lines it wants for free.
    let asked_for = config.view.explain.unwrap_or(ExplainedLines::WHOLE_FILE);
    if asked_for != ExplainedLines::WHOLE_FILE && !config.view.prints_text() {
        return refuse("'--explain' was given lines to show and '--output json' writes an entry for \
every line of the file, which is what a program reading it expects. Ask for one or the other.");
    }

    match mezura_core::explain_file(path, &config.engine, languages) {
        Ok(explanation) => {
            if config.view.prints_text() {
                print_text(&target.path, &explanation, config.view.counting, asked_for);
            } else {
                print_json(&target.path, &explanation, config.view.counting);
            }
            ExitCode::SUCCESS
        },
        Err(ExplainError::UnclaimedFile) => refuse(&format!("No language of this run claims '{}', \
so there is nothing to explain. If its language was narrowed away, drop '--languages' or \
'--exclude-languages'; if the extension is unknown, '--force-language' hands it to a language.",
                target.path)),
        Err(ExplainError::UnreadableFile(reason)) => refuse(&format!("'{}' could not be read: \
{reason}", target.path)),
        Err(ExplainError::LanguagesFromAnotherConfig) => refuse("The languages of this run were \
resolved against different settings, so the answer would be for the wrong selection."),
        // 'ExplainError' is non_exhaustive, so a reason added later stops here rather than in the
        // middle of a run
        Err(other) => refuse(&format!("'{}' could not be explained: {other}", target.path))
    }
}

fn refuse(message: &str) -> ExitCode {
    eprintln!("\n{}\n", get_active().error.paint(&wrap_message(message)));
    ExitCode::FAILURE
}

fn print_text(path: &str, explanation: &FileExplanation, model: CountingModel, asked_for: ExplainedLines) {
    let theme = get_active();
    println!("{}", theme.explain_heading.paint(&format!("{path} as {}, counted by {}",
            explanation.language, model.name())));
    println!();
    if explanation.lines.is_empty() {
        println!("{}", theme.note.paint("The file has no lines."));
        return;
    }

    let whole_file = asked_for.is_the_whole_file(explanation.lines.len());
    if !whole_file {
        println!("{}", theme.note.paint(&format!("Showing lines {} to {} of {}.", asked_for.first,
                asked_for.last.min(explanation.lines.len()), explanation.lines.len())));
        println!();
    }

    let width = explanation.lines.len().to_string().len();
    let mut printed = 0;
    for (at, (source, line)) in explanation.contents.lines().zip(&explanation.lines).enumerate() {
        if !asked_for.holds(at + 1) {
            continue;
        }
        if printed > 0 {
            println!();
        }
        printed += 1;
        let bucket = model.fold(line.class);
        let bucket_style = match bucket {
            Bucket::Code => &theme.explain_code,
            Bucket::Comments => &theme.explain_comments,
            Bucket::Third => &theme.explain_extra
        };
        println!("{:>width$}  {}", at + 1, paint_by_spans(source, &line.spans));
        let mut verdict = format!("{}  {}", bucket_style.paint(model.get_bucket_name(bucket)),
                theme.explain_detail.paint(line.class.name()));
        let mut notes = line.read_as.as_ref().map(|name| format!("read as {name}"))
                .into_iter().collect::<Vec<_>>();
        notes.extend(describe_carried(&line.carried));
        if !notes.is_empty() {
            verdict = format!("{verdict}  {}", theme.note.paint(&format!("({})", notes.join("; "))));
        }
        println!("{:>width$}  {verdict}", "");
    }

    // Two lines when a range was asked for, and the file's own is the second of them: a range total
    // alone says nothing about the count somebody opened '--explain' to check, and the file's alone
    // does not answer how much of the range is comment.
    println!();
    if whole_file {
        print_totals(&explanation.classes, explanation.lines.len(), model, "");
    } else {
        print_totals(&collect_classes_of(explanation, asked_for), printed, model, " shown");
        print_totals(&explanation.classes, explanation.lines.len(), model, " in the file");
    }
}

// The document linejudge reads: 'format', 'lines', 'buckets' and one 'per_line' entry per physical
// line are the contract, everything else is mezura's own and a reader is free to skip it.
fn print_json(path: &str, explanation: &FileExplanation, model: CountingModel) {
    let (code, comments, third) = fold_totals(&explanation.classes, explanation.lines.len(), model);
    let mut document = String::with_capacity(120 + 70 * explanation.lines.len());
    document.push_str(&format!("{{\"format\":1,\"counter\":\"mezura\",\"file\":\"{}\",",
            escape(path)));
    document.push_str(&format!("\"language\":\"{}\",\"counting\":\"{}\",\"lines\":{},",
            escape(&explanation.language), model.name(), explanation.lines.len()));
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
    println!("{document}");
}

fn print_totals(classes: &LineClasses, lines: usize, model: CountingModel, of_what: &str) {
    let theme = get_active();
    let (code, comments, third) = fold_totals(classes, lines, model);
    let label = format!("{}{of_what}", if lines == 1 {"line"} else {"lines"});
    println!("{} {}: {} {}, {} {}, {} {}",
            theme.lines_number.paint(&lines.to_string()), theme.lines_label.paint(&label),
            theme.code_number.paint(&code.to_string()), theme.explain_code.paint("code"),
            theme.comments_number.paint(&comments.to_string()), theme.explain_comments.paint("comments"),
            theme.extra_number.paint(&third.to_string()), theme.explain_extra.paint(model.get_third_quantity_name()));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
