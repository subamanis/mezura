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

Three of them, read by `test_parse_dir` in `src/language_file.rs`, which asserts that two parse and
one does not. They are inputs to the definition-file parser and have nothing to do with counting, so
they deliberately do not live beside the shipped definitions in `data/languages`.

## What is not covered here yet

The **printed output** has no golden. The binary reads its languages and its default config from the
machine's persistent app directory, so a test that ran the executable would depend on whatever the
developer has configured locally. Output goldens need the calculation/presentation split planned for
v3.0.0; until then, correctness is asserted on the stats rather than on the rendering.
