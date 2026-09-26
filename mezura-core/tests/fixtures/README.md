# Test fixtures

Everything here is a checked-in input that the tests read and never write to. A test that needs to
write goes to a temporary directory of its own; nothing writes into this one.

## `lang/`, per-language parser fixtures

One small file per language, whose **first line declares the counts mezura must produce for it**, as a
comment in that language's own syntax:

```rust
// mezura-expect lines=22 code=10 structs=1 enums=1 traits=1
```

`language_fixtures_match_their_declared_counts`, **inline in `src/engine/file_parser.rs`**, walks this
directory, resolves each file's language through the same extension mapping the program uses, parses
it, and compares. The declared numbers are **hand-verified ground truth**, not captured output: a
mismatch means either the parser regressed or the fixture is wrong, and both are worth stopping for.

It lives beside the parser rather than in `tests/` because it calls `parse_file` and `KeywordMatcher`
directly, and every file under `tests/` is a separate crate that can only reach `pub` items. Run it
with `cargo test -p mezura-core language_fixtures`.

### Adding a language

Drop in a file with the right extension and a header. No test code changes.

Rules that keep the numbers honest:

- **The header line counts.** It is a comment, so it is part of `lines` and not part of `code`.
- **Available fields:** `lines`, `code`, `extra` (which is `lines - code`), and any of that language's
  keyword names as defined in its language file, for example `structs` or `interfaces`. A field name
  that does not exist is an error, so a typo cannot silently pass.
- **Every keyword that occurs must be declared.** If a fixture produces keyword hits the header does
  not mention, the test fails, so a fixture cannot quietly stop covering a keyword.
- **One language per extension.** A separate test asserts that no fixture uses an extension claimed by
  two languages, since those counts would depend on the tie-break rule rather than on the parser.
- **A fixture with no extension resolves through its whole name or its `#!` first line**, the way
  the program resolves it. `configure` is Shell because its first line names `sh`, and that line
  also carries the expectation header, `#!/bin/sh # mezura-expect ...`, since both have to be first.
- **Keep them small enough to count by hand.** A fixture nobody can verify is worse than no fixture.
- **Avoid constructs whose intended behavior is unsettled**, such as Python triple-quoted strings,
  which mezura's string-symbol model does not describe. Encoding a guess as ground truth is how a test
  suite starts lying.

Worth covering per language: a full-line comment, a trailing comment, a blank line, a brace-only line
(which is not code by default), comment markers inside strings, and each keyword at least once.

## `stats.golden`

The byte-for-byte record of what the whole `lang/` corpus produces, checked by
`tests/stats_golden.rs`, which calls `run` over it with one producer and four consumer threads.

Regenerate after an intentional change and review the diff before committing it:

```
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura-core --test stats_golden
```

Byte sizes are deliberately not in the report: they are the one figure that differs between a CRLF and
an LF checkout, which would break the golden on the CI matrix.

## `parser/`, the four sample files of the parser cases

`a.txt` to `d.txt`, read by the parser tests inline in `src/engine/file_parser.rs`. Unlike `lang/`,
these are not one per language and they declare nothing: the language is the one the test names, and
the expected counts live in the test body. That is the point of them, and why they carry a `.txt`
extension that implies no language: the same file is counted as Java and then as C#, which is how the
per-language rules are told apart from the parser's own mechanics.

## `definitions/`, language definition files

Three of them, read by `a_stray_line_in_one_definition_costs_that_language_and_no_other` in
`src/language_file.rs`. The C++ one is correct everywhere except for one stray line under the
language name, which is the mistake somebody editing a file by hand makes; the other two have to
come through it untouched. They are inputs to the definition-file parser and have nothing to do with
counting, so they deliberately do not live beside the shipped definitions in `data/languages`.

## `test_code/`, one tree per language for the test detection

Each directory is a small project of one language, and its `expected.txt` says which lines of every
file in it are test code. `tests/test_detection.rs` copies each tree to a temporary directory,
counts it through `run` with one producer and four consumers and `collect_files` on, and compares
every file and the language's own figure against that file. The copy is not optional: the fixtures
sit under the `tests` beside this crate's `Cargo.toml`, where every file is test code. Each tree
is copied twice, under a folder named `tests` and under an ordinary one, and both copies have to
answer the same. A build file that cargo would read as a package of its own, the Rust tree's
`Cargo.toml`, is checked in as `Cargo.toml.fixture` and copied without the suffix, since
`cargo package` leaves a directory holding a `Cargo.toml` out of the crate.

The file is a `[Language]` heading and then one line per file, the path and then what is test code
in it:

```
[Rust]
src/inline_module.rs 3-11
src/attributed_functions.rs 3-6,10-14,16-19
tests/integration.rs whole
src/markers_elsewhere.rs none
src/attribute_on_a_field.rs 3-6 # wrong, the field alone is 3-4
```

Line ranges are hand verified by opening the file, which is the point of writing them as lines. The
test turns them into a line count and a byte count by reading the file itself, so a checkout with
other line endings changes nothing. Every file of the tree needs a line, and a file counted under
another language fails, so a fixture whose content does not identify its language is caught.

A `# wrong` note marks a known limitation and states what mezura answers today. The fix that
changes the answer breaks the test and removes the note. What each language finds and what it does
not is in `TEST_DETECTION.md` at the repository root.

The three languages with markers, Rust, D and Zig, carry every form the extent rule has to follow
and every trap it is known to fall into, written adversarially: a test module that is the last thing
in a file proves nothing about the rule. Go and Perl hold a file of the name their toolchain defines
and one that matches none. The other trees hold a build file with the directory it makes test code,
`sbt` and `gradle` a build declared from its root whose subprojects carry no build file of their
own, and `no-build-file` holds those directories with nothing beside them.

## The printed output, which is covered elsewhere

`mezura/tests/fixtures/layouts.golden` is the same idea for the presentation, and it lives in the
other package because that is where the printing does. It renders one fixed dataset through every
layout and is regenerated with `MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura every_layout`.
