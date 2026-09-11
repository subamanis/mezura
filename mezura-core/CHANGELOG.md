# Changelog

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
