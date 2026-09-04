# Contributing

## What the project is

mezura counts lines of code. You point it at a folder, it walks the tree, reads each file and prints
how many lines are code, how many are comments and how many are neither.

Three crates in one repository:

- `mezura-core/` does the counting. It is a library, and other programs can use it.
- `mezura/` is the command line: flags, colours, tables, configuration files, JSON output.
- `mezura-mcp/` lets a coding assistant run mezura. A separate binary that starts the command line
  as a child process and hands back its answer. It links neither of the other two, so installing
  mezura does not pull it in.

A change about what a number should be belongs in `mezura-core`. A change about what the user sees
or types belongs in `mezura`.

## Getting it running

```
cargo build --release
cargo run --release -p mezura -- ./some/folder
```

The first run writes the language definitions, the themes and the default configuration into your
user directory:

```
Windows   %APPDATA%\mezura\data\
Linux     ~/.local/share/mezura/
macOS     ~/Library/Application Support/mezura/
```

## Before you open a pull request

CI runs these three, so if they pass here they pass there.

```
cargo build --release
cargo clippy --workspace --all-targets -- -D warnings -A clippy::too_many_arguments
cargo test
```

`too_many_arguments` is the one warning left switched off on purpose. Everything else has to be
clean.

## Adding a language

A language is a text file, so adding one needs no code.

1. Copy the closest file in `mezura-core/data/languages/` and edit it. **[The language files
   guide](LANGUAGE_FILES_GUIDE.md)** walks through every line of it.
2. Add one small sample file to `mezura-core/tests/fixtures/lang/`. Its first line says what the
   counts must be, like `// mezura-expect lines=22 code=10`. Count them by hand. See
   [the fixtures README](mezura-core/tests/fixtures/README.md).
3. Run `cargo build --release && cargo test`. The build comes first because the MCP server
   starts mezura as a separate process, and `cargo test` never builds the binary of another
   package.

The sample should look like the language normally looks. Anything strange, a comment symbol hiding
inside a string, an unbalanced quote, a comment that never closes, is a case for
[LineJudge](https://github.com/loc-conformance/linejudge), the conformance corpus every counter is
measured against. CI runs it on every change, holding the build to `.linejudge/recorded/mezura.toml`;
a fix that moves an answer regenerates that file in the same commit with
`linejudge record --counter mezura --bin target/release/mezura`.

## Tests

Tests sit at the bottom of the file they test, in a `#[cfg(test)] mod tests` block. Put new ones
there.

Four files hold recorded output that a test compares against. When a deliberate change makes one of
them fail, regenerate it and **read the diff before you commit it**:

```
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura-core --test stats_golden   counts over the whole corpus
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura every_layout               how each report layout draws
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme                     the command list in the README
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura commands_document          COMMANDS.md
```

## Things that will send a change back

**Comments.** The default is no comment. Write one only when the code cannot say the thing itself:
an ordering that matters, a constraint, a trap somebody will otherwise fall into. One or two lines.
Never a comment that repeats the name of the function under it, and never a note about what the code
used to do.

**Changelog.** Anything a user can notice gets one line in `mezura/Changelog`, written for the person
running the program. The reasoning behind it belongs in the pull request.

**Help texts and the README go together.** The command list in the README and `COMMANDS.md` are both
generated from the help strings in `mezura/src/message_printer.rs`. Change a help text, then run the
two commands above. A test fails if you forget.

**Commit messages are one line.** Look at `git log` for the style. A body only when the change really
cannot be named in a line, and then a few lines.

## Two traps

**Editing a language file in the repository does not change what the program reads.** The running
binary reads the copy in your user directory (the paths above). The repository copy is the template
installed on first run. To test a change to a language file, copy it across, or run
`mezura --restore`, which rewrites your data directory from the built binary and keeps whatever you
had edited under `replaced/`. Meanwhile `cargo test` reads the repository copy, so the two can
disagree and nothing tells you.

**Colours cannot be tested through printed text.** A test binary is not a terminal, so nothing is
painted and asserting on the printed string proves nothing. Assert on the style itself, through
`fgcolor()` and `style()`.

## Reporting a bug

For a count that looks wrong:

```
mezura path/to/the/file --explain
```

That prints the file line by line with what mezura decided about each one and why. Paste it, along
with the file if you can share it.

## Licence

By contributing you agree that your work is released under the same terms as the project, MIT or
Apache-2.0 at the user's option.
