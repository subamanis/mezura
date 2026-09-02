# mezura-core

[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/subamanis/mezura#license)

The fast, multithreaded counting engine behind
[mezura](https://github.com/subamanis/mezura): identifies each file's language, splits every line
into code, comments and everything else, and counts user-defined keywords like classes and structs.

Over eighty languages ship with it, identified by extension, by whole file name and by the `#!` line of a
script. It leaves out what a `.gitignore` leaves out, counts the `<script>` and `<style>` sections
of a page under their own languages, and reads a file properly enough that a comment symbol inside
a string is not mistaken for a comment. An extension claimed by more than one language is resolved
by reading the file's own content, and a file wearing a language's extension without holding code,
like a `.d` dependency file, is set aside. `EngineConfig::use_heuristics` turns off both of those
readings, and `count_not_code` counts such a file anyway while leaving the identification in place.
Bundled and generated files are set aside the same way, under `count_minified` and
`count_generated`, and `RunResult::skipped_files` names every file that was, by the reason it was
set aside for.

This is the library. For the command line program, the report it prints and the settings it takes,
see the [main README](https://github.com/subamanis/mezura).

Counted by itself, on 2026-09-02:

```
Details.

Language   Files %      ⌄ Lines %       Code %       Comments %       Extra       Size
──────────────────────────────────────────────────────────────────────────────────────
Rust          22 100%    13,509 100%   8,144 60.3%      2,187 16.2%   3,178   685.4 KB

Keywords.

Rust   enums: 21, structs: 69
```

## Counting a directory

```rust
use mezura_core::{CountingModel, EngineConfig, Languages, run};

let config = EngineConfig::new(["./src", "./tests"]);
let (languages, warnings) = Languages::shipped(&config);
for warning in &warnings {
    eprintln!("{}", warning.message);
}

let result = run(&config, languages)?;
for (name, stats) in result.sort_languages_by(Default::default(), CountingModel::Content) {
    println!("{name}: {} code", stats.calculate_code_lines(CountingModel::Content));
}
```

`EngineConfig` says what to count. `Languages` says what the symbols of each language are, and is
built against that same configuration: a run refuses a pair that does not match, since counting
Rust with settings that name Python would give figures that look perfectly normal and describe
something else.

## Two answers from one run

Every line is sorted into one of nine classes, and a `CountingModel` folds those nine into the three
columns of a report.

`Content` asks what a line says: words in code make it code, words only in a comment make it a
comment, and punctuation and blank lines are neither. `Region` asks where a line sits, which is how
cloc, tokei and scc count: any code on the line makes it code, and a line inside a comment belongs
to the comment whatever it holds.

Both come out of the same run, so switching costs no recounting.

## Also here

`run_watched` is the same run for a caller that needs real time feedback while it happens, and
`explain_file` reads a single file line by line and says why each line was counted the way it was:
the class it landed in, what earlier lines had left open, and which stretches of it sit inside a
string or a comment. It also says what identified the file when its extension is contested, and why
a directory scan would have left it out.

Languages of your own go in through `Languages::shipped_with`, and a caller keeping its own
directory of language files parses them with `language_file` and hands them to `Languages::resolve`.

The full API is on [docs.rs](https://docs.rs/mezura-core).

## License

MIT OR Apache-2.0, at your option.
