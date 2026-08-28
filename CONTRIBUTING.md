# Contributing

Thanks for looking. This page is what you need to get a change in, and nothing else.

## What the project is

mezura counts lines of code. You point it at a folder, it walks the tree, reads each file and prints
how many lines are code, how many are comments and how many are neither.

It is three crates in one repository:

- `mezura-core/` does the counting. It is a library, and other programs can use it.
- `mezura/` is the command line: flags, colours, tables, configuration files, JSON output.
- `mezura-mcp/` lets a coding assistant run mezura. It is a small separate binary that starts the
  command line as a child process and hands its answer back, and it links neither of the other two,
  so nobody installing mezura pays for it.

If your change is about what a number should be, it belongs in `mezura-core`. If it is about what
the user sees or types, it belongs in `mezura`.

## Getting it running

```
cargo build --release
cargo run --release -p mezura -- ./some/folder
```

The first run writes a folder of data files (the language definitions, the themes, the default
configuration) into your user directory. Where it lands:

```
Windows   %APPDATA%\mezura\data\
Linux     ~/.local/share/mezura/
macOS     ~/Library/Application Support/mezura/
```

## Before you open a pull request

Three commands. CI runs the same ones, so if they pass here they pass there.

```
cargo build --release
cargo clippy --workspace --all-targets -- -D warnings -A clippy::too_many_arguments
cargo test
```

`too_many_arguments` is the one warning we leave switched off on purpose. Everything else has to be
clean.

## Adding a language

This is the most common change and the easiest one. A language is a text file, and adding one needs
no code at all.

1. Copy the closest file in `mezura-core/data/languages/` and edit it. **[The language files
   guide](LANGUAGE_FILES_GUIDE.md)** walks through every line of it.
2. Add one small sample file to `mezura-core/tests/fixtures/lang/`. Its first line says what the
   counts must be, like `// mezura-expect lines=22 code=10`. Count them by hand. See
   [the fixtures README](mezura-core/tests/fixtures/README.md).
3. Run `cargo build --release && cargo test`. The build comes first because the MCP server
   starts mezura as a separate process, and `cargo test` never builds the binary of another
   package.

The sample file should look like the language normally looks. Anything weird, a comment symbol
hiding inside a string, an unbalanced quote, a comment that never closes, goes to `stress-corpus/`
instead, where the tricky cases live.

## Tests

Most tests sit at the bottom of the file they test, in a `#[cfg(test)] mod tests` block. That is
where to put a new one.

Three files hold recorded output that tests compare against. When you change something on purpose
and one of them fails, regenerate it and **read the diff before committing it**:

```
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura-core --test stats_golden   counts over the whole corpus
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura every_layout               how each report layout draws
MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme                     the command list in the README
```

## Things that will send a change back

**Comments.** The default is no comment. Write one only when the code cannot say the thing itself:
an ordering that matters, a constraint, a trap somebody will otherwise fall into. One or two lines.
Never a comment that repeats the name of the function under it, and never a note about what the code
used to do.

**Changelog.** Anything a user can notice gets one line in `mezura/Changelog`, written for the person
running the program: what changed for them, not how it was done.

**Help texts and the README go together.** The command list in the README is generated from the help
strings in `mezura/src/message_printer.rs`. Change the help, then run
`MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme`. A test fails if you forget.

**Commit messages are one line.** Look at `git log` for the style. No body unless the change really
cannot be named in a line, and then a few lines, not a page.

## Two things that waste people's afternoons

**Editing a language file in the repository does not change what the program reads.** The running
binary reads the copy in your user directory (the paths above). The repository copy is only the
template that gets installed on first run. To test a change to a language file, copy it across, or
run `mezura --restore`, which rewrites your data directory from the built binary and keeps whatever
you had edited under `replaced/`. Meanwhile `cargo test` reads the repository copy, so the two can
disagree and nothing tells you.

**Colours cannot be tested through printed text.** A test binary is not a terminal, so nothing is
painted and asserting on the printed string proves nothing about colour. Assert on the style itself
instead, through `fgcolor()` and `style()`.

## Reporting a bug

A count that looks wrong is the most useful bug report there is, and the fastest way to show one is:

```
mezura path/to/the/file --explain
```

That prints the file line by line with what mezura decided about each one and why. Paste that, plus
the file if you can share it, and the problem is usually obvious from it.

## Licence

By contributing you agree that your work is released under the same terms as the project, MIT or
Apache-2.0 at the user's option.
