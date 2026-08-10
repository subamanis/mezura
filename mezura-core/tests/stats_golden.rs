use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mezura_core::{EngineConfig, Languages, Stats, language_file, run};

const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");

const CONSUMER_THREADS: usize = 4;
const UPDATE_ENV_VAR: &str = "MEZURA_UPDATE_GOLDEN";

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("lang")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("stats.golden")
}

// Byte sizes are deliberately absent from the report: they are the one figure that differs between a
// CRLF and an LF checkout of the same fixtures, which would break the golden on the CI matrix.
fn render_report(per_language: &HashMap<String, Stats>) -> String {
    let mut names = per_language.keys().cloned().collect::<Vec<_>>();
    names.sort();

    let (mut total_files, mut total_lines, mut total_code, mut total_comments) = (0, 0, 0, 0);
    let mut report = String::with_capacity(500);
    for name in &names {
        let info = per_language.get(name).unwrap();
        total_files += info.files;
        total_lines += info.lines;
        total_code += info.code_lines;
        total_comments += info.comment_lines;

        report.push_str(&format!("{name}\n  files={} lines={} code={} comments={}\n",
                info.files, info.lines, info.code_lines, info.comment_lines));

        let mut keywords = info.keyword_occurences.iter().collect::<Vec<_>>();
        keywords.sort_by_key(|(name, _)| name.as_str());
        if !keywords.is_empty() {
            let rendered = keywords.iter().map(|(name, count)| format!("{name}={count}")).collect::<Vec<_>>();
            report.push_str(&format!("  {}\n", rendered.join(" ")));
        }
    }

    format!("files={total_files} lines={total_lines} code={total_code} comments={total_comments}\n\n{report}")
}

// One producer and several consumers, which is where the per-thread stats merging happens and where
// the historical nondeterminism bugs lived. Through 'run' and not through a hand-built queue, so
// that what is measured is the wiring the program actually uses.
fn collect_stats() -> String {
    // Only the counting half, since nothing here is printed.
    let config = EngineConfig {
        threads: mezura_core::Threads::new(1, CONSUMER_THREADS),
        ..EngineConfig::new([fixtures_root().to_str().unwrap().replace('\\', "/")])
    };

    // The repository's own files and not the copies baked into the library, so that editing one of
    // them and rerunning the golden measures the edit.
    let languages_on_disk = language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0;

    let (languages, _) = Languages::resolve(&config, languages_on_disk, &Default::default());

    // The all-faulty guard lives on the asserts below: that case comes back as a result now, and
    // the empty faulty list is already demanded a few lines down
    let result = run(&config, languages, None, |_| {})
            .unwrap_or_else(|x| panic!("the fixture corpus could not be counted: {x:?}"));

    assert!(result.files_present.relevant_files > 0, "the fixture corpus produced no relevant files");
    assert!(result.faulty_files.is_empty(), "{} fixture(s) failed to parse", result.faulty_files.len());

    render_report(&result.per_language)
}

#[test]
fn stats_of_the_fixture_corpus_match_the_golden_byte_for_byte() {
    let report = collect_stats();
    let golden = golden_path();

    if std::env::var(UPDATE_ENV_VAR).is_ok() {
        std::fs::write(&golden, &report).unwrap();
        println!("{} was set, rewrote {}", UPDATE_ENV_VAR, golden.display());
        return;
    }

    // The line endings are normalised on the way in, the way the layouts golden beside this one
    // already does it. The file is written with '\n' and git hands it back with '\r\n' on a Windows
    // checkout, so the two differ in every line ending and in nothing else: the comparison failed
    // while every line matched, and a fresh clone on this platform failed on its first run.
    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|x|
            panic!("cannot read {}: {x}\nRun with {UPDATE_ENV_VAR}=1 to create it.", golden.display()))
            .replace("\r\n", "\n");

    if expected != report {
        let mut differences = Vec::new();
        let (expected_lines, actual_lines) = (expected.lines().collect::<Vec<_>>(), report.lines().collect::<Vec<_>>());
        for i in 0..expected_lines.len().max(actual_lines.len()) {
            let (before, after) = (expected_lines.get(i).unwrap_or(&"<missing>"), actual_lines.get(i).unwrap_or(&"<missing>"));
            if before != after {
                differences.push(format!("line {}: expected \"{before}\", got \"{after}\"", i + 1));
            }
        }
        // Two files can differ while no line of one differs from the same line of the other, since
        // 'lines' hands back neither the ending nor a final empty line. Left unsaid, the report was
        // a heading with nothing under it and no way to guess what had changed.
        if differences.is_empty() {
            differences.push(format!("no line differs, so the two differ only in whitespace: {} bytes recorded against {} produced",
                    expected.len(), report.len()));
        }
        panic!("\nthe fixture corpus no longer produces the recorded stats:\n  {}\n\nIf the change is intended, rerun with {UPDATE_ENV_VAR}=1 and review the diff of {}.\n",
                differences.join("\n  "), golden.display());
    }
}

#[test]
fn two_runs_over_the_same_corpus_produce_identical_stats() {
    assert_eq!(collect_stats(), collect_stats(), "the same corpus produced different stats across two runs in the same process");
}
