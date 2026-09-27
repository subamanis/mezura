# Changelog

## 2.0.0, unreleased

New:

- Test code can now be detected. `RunResult::tests`, `ModuleResult::tests` and
  `FileEntry::tests` carry it per language, per module and per file, as a `TestCode` holding the
  same `Stats` a language has plus `whole_files`. Without being asked the engine takes what a
  language's markers open (Rust, D and Zig), the file names a toolchain defines (`_test.go`,
  Perl's `.t`) and the directory a build tool compiles for tests alone beside its build file;
  `EngineConfig::test_patterns` declares the rest, a `!` pattern takes a file or a directory back,
  and `EngineConfig::detect_tests` turns the whole of it off. What each language gets right and
  what it misses is in `TEST_DETECTION.md` beside the main README.
- `Language::test_markers`, `Language::test_file_names` and `Language::with_tests`, read from the
  `Tests` block of a language file, and `TestFileName` for the three shapes a name takes.
- `PathPatterns`, the patterns of the exclusions and of the test code together with the directory
  their names are read from, and `PatternError` for one that does not parse. The warning code
  `pattern-matched-nothing` reports a pattern that names nothing on disk, a place outside every
  target, or a name nothing matched.
- `FileExplanation::test_file_rule` names the rule that made a whole file test code, as a
  `TestFileRule`, and `ExplainedLine::in_test` marks each line of it.
- `.t` files are counted as Perl.

Changed:

- `EngineConfig::exclude_dirs` is now `exclude_patterns`, a `PathPatterns`. A name pattern matches
  at any depth below the folder that holds the target and no folder above it, a pattern starting
  with `./` or `../` or written as a full path names one place, a trailing `/` means a folder
  only, `x/**` means the folder `x`, and names are matched with their case.
- `EngineConfig`, `Language`, `ExplainedLine`, `FileExplanation`, `RunResult`, `ModuleResult` and
  `FileEntry` gained public fields, so a struct literal of any of them written outside the crate
  stops compiling until it names them.

Fixes:

- An absolute target holding `..`, `D:/proj/tests/..`, is resolved to the folder it names, so
  beside a target inside that folder it is counted once. A relative target on a network share,
  `./src` from `\\server\share\proj`, no longer fails.
- An exclusion no longer matches a folder above the counted tree, so `tests/**` no longer drops a
  project that sits under a folder named `tests`, and a pattern starting with `./` excludes what
  it names. A full path pattern works under a folder with brackets in its name and in any letter
  case.

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
