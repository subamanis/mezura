# Changelog

## 2.0.0, 2026-10-29

New:

- Test code detection. Each language's tests are counted apart, as a `TestCode` per language in
  `RunResult::tests` and `ModuleResult::tests` and per file in `FileEntry::tests`. It is on by
  default (`EngineConfig::detect_tests`), `EngineConfig::test_patterns` adds or takes back files and
  folders and disables a preset with `!@<name>`, and a test tool's default names apply where its
  configuration file is found (`TestPreset`, `get_test_presets`, `RunResult::applied_presets`). What
  each language detects is in `TEST_DETECTION.md`.
- `explain_file` marks every line of test code (`ExplainedLine::in_test`) and names the rule that
  made a whole file test code (`FileExplanation::test_file_rule`, a `TestFileRule`).
- `PathPatterns`, one type for the exclusion and the test patterns, with `PatternError` for one that
  does not parse, and `RunResult::warnings` for a pattern that matched nothing.
- New shapes a language file can declare: heredocs, strings with a delimiter of their own (C++'s
  `R"x(`), strings whose ends carry a counted run (Rust's `r#"`, Lua's `[==[`), quotes escaped by
  doubling, comments that count only at the start of a line (Perl's POD, Ruby's `=begin`) or as a
  whole line (MATLAB's `%{`), and block comments with one symbol at both ends. CoffeeScript and
  Smalltalk ship as languages, and `.t` files count as Perl.
- `Display`, `FromStr` and `Hash` on most public enums, as docs.rs lists.

Breaking:

- Renamed:
  - `Stats::keyword_occurences` to `keyword_occurrences`, `Stats::total_of` to `calculate_total_of`
  - `name` to `as_str` on `Code`, `Affects`, `CountingModel`, `SortCriterion`, `ScanSkip`,
    `SpanKind`, `LineClass` and `ConflictBlock`
  - `Threads::producers` and `consumers` to `get_producers` and `get_consumers`
  - `LanguageClaims::get_contested_claims` to `collect_contested_claims`,
    `ScopedByModule::get_all_names` to `collect_all_names`
  - `NumberFormat`'s `integer`, `grouped`, `size_with_unit`, `percent`, `signed_percent` and
    `with_decimal_mark` to `format_integer`, `group_digits`, `format_size`, `format_percent`,
    `format_signed_percent` and `replace_decimal_mark`
  - `LanguageDirParseError::PathMissing` to `Unreadable`
- `EngineConfig::exclude_dirs` is `exclude_patterns`, a `PathPatterns`, and a `!` pattern in it is
  refused.
- The `engine` module is private. What was used from it sits at the crate root, and the thread
  limits are `Threads::MAX_PRODUCERS`, `MIN_PRODUCERS`, `MAX_CONSUMERS` and `MIN_CONSUMERS`.
- `validate_exclude_patterns` moved to the crate root and returns a `PatternError`, which
  `RunError::InvalidExcludePattern` now carries.
- `EngineConfig`, `Language`, `ExplainedLine`, `FileExplanation`, `RunResult`, `ModuleResult`,
  `FileEntry` and `MultilineString` have new public fields, so a struct literal of one of them
  needs them.
- `FaultyFileDetails` and `UnreadableDirDetails` are built with a struct literal; their `new` is gone.
- `LeveledPair` is built with `LeveledPair::parse`, and `MultilineString`'s `escaping`, `raw` and
  `of` are private, with the `with_` methods of `StringRules` building every shape. A
  `Carried::Str` opener is spelled with its run, `r##"`.
- `parse_language` returns a `Result`.
- `ExplainError::UnreadableFile` and `LanguageDirParseError::Unreadable` carry the `io::Error`,
  `ExplainError` is no longer `PartialEq`, and `RunError::source` gives nothing.
- `warnings::Code` keeps only the codes this crate raises; the nine that only the mezura program
  raises left it.
- `Language::with_line_continuation` takes a `LineContinuation`, and `run_watched` takes an
  `Option<&ScanProgress>`.

Fixes:

- A target written with `..` in it is counted once beside a target inside it, and a relative
  target on a network share works.
- An exclusion no longer matches a folder above the counted tree, so `tests/**` keeps a project
  that sits under a folder named `tests`.
- Comment symbols that share bytes, `/* x /*/` in C or `*/*/*/` in Rust, are read the way the
  language reads them.
- Block comments nest in Dart, WGSL, Jai and Pascal.
- A keyword with a tab before or after it is counted.
- `explain_file` uses the module and the test patterns of the target holding the file, as `run`
  does.
- An empty symbol given to a builder of `Language`, `StringRules` or `Keyword` is dropped, where it
  stopped the run.
- Smaller ones: two `Language`s differing only in cancelled symbols or identifying lines no longer
  compare equal, `parse_conflict_rules_file` keeps its rules past a byte that is not UTF-8, and a
  language given to `Languages::shipped_with` under a shipped name replaces it without a warning.

-----------------------------------------------------------------------------------------------------------

## 1.1.2, 2026-09-19

14,826 Total lines  -  8,564 Code lines

Fixes:

- A string opener written inside a comment, such as Rust's 'r#"' or C++'s 'R"(', no longer
  swallows the rest of that line.

Other:

- The crate no longer forbids unsafe code: `#![forbid(unsafe_code)]` became `#![deny(unsafe_code)]`,
  with four small unsafe blocks: two calls into the AVX2 routines (NEON on arm64), one reading a file
  that passed the ASCII check as text without a second validation, and one mapping a large file into
  memory.
- Counting is measurably faster. Each file is split into lines and scanned for its symbols in one pass over
  64-byte blocks, where it used to take a search per line. On arm64, Apple Silicon included, that pass
  runs with NEON where it ran byte by byte. On Linux the walk no longer asks the size of every file,
  files of 256 KB and up are mapped into memory (mmap), and a file whose bytes are all ASCII skips the
  UTF-8 validation.

-----------------------------------------------------------------------------------------------------------

## 1.1.1, 2026-09-11

14,126 Total lines  -  8,532 Code lines

Fixes:

- A file larger than 2 GB is counted whole on Linux and macOS, where it was being cut short at 2 GB.

-----------------------------------------------------------------------------------------------------------

## 1.1.0, 2026-09-11

14,125 Total lines  -  8,531 Code lines

New:

- `LanguageClaims` says which language owns an extension, a whole file name or the interpreter a
  `#!` line names. Each `Claim` carries the owner, the languages that claimed it and did not get it,
  and what settled it: a `--force-language` pair, a line of `language_conflicts.txt`, or the
  alphabet. That last one is the case nobody chose, and the losers' comment symbols are then wrong.
- `EngineConfig::detect_shebangs` decides whether a file with no extension is identified by the
  interpreter its `#!` line names. Set to false, such a file is never opened and never counted, and
  no `#!` line decides anything, a contested extension included.
- `Language::with_cancelled_symbols` says that a symbol does not count when a given character sits
  right in front of it. C3 needs it, since the `<*` in `int[<*>]` opens nothing.
- Mercury and C3 added to the shipped languages (83 in total now).

Fixes:

- A keyword repeated back to back with nothing between the repeats counts as nothing, and a file
  holding a long run of them no longer takes minutes to count.
- Files that could not be parsed and directories that could not be read come back in path order.
- A file that went away while it was being counted keeps its listed size.

Other:

- A `%` comment no longer identifies a MATLAB file, since every Mercury file opens with one. MATLAB
  answers to `%%`, `function` and `classdef` now.
- Counting a large tree is about a fifth faster.

-----------------------------------------------------------------------------------------------------------

## 1.0.0, 2026-09-02

13,509 Total lines  -  8,144 Code lines

The first release: the counting engine of mezura as a library. `run` counts a directory and
answers per language, per module and per file, `explain_file` reads one file line by line and says
why each line was counted the way it was, and `Languages` holds the over eighty shipped languages
and takes yours beside them. Every line is sorted into one of nine classes, and both counting
models, content and region, come out of one run.

The version moves with the library's API. The mezura program built on it has a version of its own.
