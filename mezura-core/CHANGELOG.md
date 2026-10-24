# Changelog

## 2.0.0, unreleased

New:

- Test code can now be detected. `RunResult::tests` and `ModuleResult::tests` carry it per
  language and per module, as a `TestCode` holding the same `Stats` a language has plus
  `whole_files`, and `FileEntry::tests` carries it per file as a `Stats`. Without being asked the
  engine takes what a language's markers open (Rust, D and Zig), the files a test annotation in
  their code makes test code whole (Java, Kotlin and C#), the file names a toolchain defines
  (`_test.go`, Perl's `.t`) and the directory a build tool compiles for tests alone beside its
  build file; `EngineConfig::test_patterns` declares the rest, a `!` pattern takes a file or a
  directory back, and `EngineConfig::detect_tests` turns the whole of it off. What each language
  gets right and what it misses is in `TEST_DETECTION.md` beside the main README.
- `TestCode::total_of` adds every language's test code together, `TestCode::add` adds one to
  another, and `TestCode::calculate_production_of` gives what a language holds outside its test
  code. `RunResult` and `ModuleResult` give the same per language and in total with
  `calculate_production_per_language` and `calculate_production_total`, and a `FileEntry` gives
  its own with `calculate_production`, and `is_tests_whole` says whether all of it is test code.
- `Language::test_markers`, `Language::test_file_names`, `Language::with_test_markers` and
  `Language::with_test_file_names`, read from the `Tests` block of a language file, and
  `TestFileName` for the three shapes a name takes.
  `Language::module_keyword` and `Language::with_module_keyword` carry the word that declares a
  module living in another file, `mod` for Rust, so that a file declared on a line of test code
  is test code whole. `Language::test_file_markers` and `Language::with_test_file_markers` carry
  the annotations that make a whole file test code, `@Test` for Java, read from `FILE MARKERS`.
- `PathPatterns`, the patterns of the exclusions and of the test code together with the directory
  their names are read from, and `PatternError` for one that does not parse. The warning code
  `pattern-matched-nothing` reports a place missing from disk or outside every target, and a name
  with a slash that nothing matched. `validate_test_patterns` checks a list of test patterns as it
  is typed, and `engine::path_patterns::is_a_path_pattern`, `absolutize_path_pattern`, `is_on_disk`
  and `split_negation` beside it read the place a pattern names.
- `FileExplanation::test_file_rule` names the rule that made a whole file test code, or the `!`
  pattern that took it back, as a `TestFileRule`, and `ExplainedLine::in_test` marks each line of
  test code. `TestFileRule::DeclaredModule` names the files that declare it as a module, the one
  holding the marker last, `TestFileRule::FileMarker` the annotation found in its code with its
  line, and `TestFileRule::InnerAttribute` the `#![cfg(test)]` standing before every item of it.
- `.t` files are counted as Perl.
- `MultilineString::escapes_by_doubling` and `StringRules::with_string_pairs_escaped_by_doubling`,
  read from `Paired strings escaped by doubling`, for a pair whose closer written twice is one quote
  of the text, `""` inside C#'s and F#'s `@"..."`.
- `StringRules::with_leveled_pairs` and `get_leveled_pairs`, a string pair whose ends carry a
  counted run, read from a `*` in `Paired string openers`: Rust's `r#*"` with `"#*` and Lua's `[=*[`
  with `]=*]`. `LeveledPair::of` reads the byte of the run from the written form, so a comment pair
  may count a character other than `=`, and a closer may end in its run.
- A plain block comment pair may carry the same symbol at both ends, Smalltalk's `"` and
  CoffeeScript's `###`, and the next one met inside the block closes it. CoffeeScript and
  Smalltalk ship as languages. `comment-pair-never-closes` now reports only a self-nesting pair
  written that way.
- `Language::heredoc` and `Language::with_heredoc`, read from `Heredoc`, naming the `HeredocRules`
  a language's heredoc follows: the shell's, Perl's, Ruby's or PHP's. After the opener a word chosen
  on the spot opens a string that ends at the line holding that word.
- `MultilineString::delimited` and `StringRules::with_delimited_string_pairs`, read from `Paired
  strings with a delimiter`, for a pair whose writer may put a delimiter of up to sixteen
  characters inside both ends, C++'s `R"js(` closing only at `)js"`.
- `Language::line_start_comments` and `Language::with_line_start_comments`, read from `Line-start
  comment start` and `Line-start comment end`, comment blocks whose ends count only at the start of
  a line, Perl's POD from `=head1` to `=cut` and Ruby's `=begin` to `=end`. `Language::whole_line_comments`
  and `Language::with_whole_line_comments`, read from `Whole-line comment start` and `Whole-line
  comment end`, blocks whose ends count only as the whole of a line and nest, MATLAB's `%{` and `%}`.
  `Carried::Comment` names the start of either as the opener and carries the depth.

Changed:

- The fields of `LeveledPair` are no longer public; `LeveledPair::of` is the way to build one. A
  `Carried::Str` opener is spelled with its run, `r##"` for a string opened that way.

- `EngineConfig::exclude_dirs` is now `exclude_patterns`, a `PathPatterns`. A name pattern matches
  at any depth below the folder that holds the target and no folder above it, a pattern starting
  with `./` or `../` or written as a full path names one place, a trailing `/` means a folder
  only, `x/**` means the folder `x`, and names are matched with their case.
- `validate_exclude_patterns` moved from `engine::targets` to the crate root, beside
  `validate_test_patterns`. It returns a `PatternError` now, and it refuses a `!` pattern.
- `EngineConfig`, `Language`, `ExplainedLine`, `FileExplanation`, `RunResult`, `ModuleResult`,
  `FileEntry` and `MultilineString` gained public fields, so a struct literal of any of them
  written outside the crate stops compiling until it names them.

Fixes:

- An absolute target holding `..`, `D:/proj/tests/..`, is resolved to the folder it names, so
  beside a target inside that folder it is counted once. A relative target on a network share,
  `./src` from `\\server\share\proj`, no longer fails.
- An exclusion no longer matches a folder above the counted tree, so `tests/**` no longer drops a
  project that sits under a folder named `tests`, and a pattern starting with `./` excludes what
  it names. A full path pattern works under a folder with brackets in its name and in any letter
  case.
- Comment symbols sharing bytes are read by the rules of the file's language: `/* x /*/` in C and
  `*/*/*/` closing three nested levels in Rust end the comment there, a `*//` in code starts a line
  comment, a `//*` inside a Rust comment opens one more level, and a `/*` after two stray `*/` in
  code opens its comment.
- Dart, WGSL, Jai and Pascal declare their block comments as nesting, which all four are, Pascal
  the way Free Pascal reads it.

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
