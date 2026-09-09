# Language choices

Which language gets an extension that more than one of them claims, and which files are left out of
the count. The shares are GitHub code search totals, taken 2026-09-01.

## Contested extensions

| Extension | Goes to | On GitHub |
|---|---|---|
| `.h` | C Header | 55% C, 36% C++, 9% Objective-C |
| `.m` | MATLAB | 59% MATLAB, 41% Objective-C, and a rounding error of Mercury. Objective-C also has `.mm`, MATLAB and Mercury have nothing else |
| `.pl` | Perl | 91% Perl, 9% Prolog. Prolog also has `.pro` and `.prolog`, and even `.pro` is Prolog only 1% of the time (67% is IDL) |
| `.pas` | Pascal | Delphi also has `.dpr` and `.dpk` |

The table is the fallback. A file is identified by its own content first: the `#!` line, then the
evidence its candidate languages declare, so a `.m` opening with `@interface` is Objective-C, one
opening with `function` is MATLAB, and one opening with `:- module` is Mercury. `--no-heuristics`
turns that off, and `--no-shebang` takes the `#!` line out of it on its own. Mercury is the reason the table matters least for `.m`: its files carry a
`:- module` declaration or a `ft=mercury` modeline at the top, and over the Mercury compiler's own
4,164 files that evidence wins all but one of them.

None of the four is right for everybody. Reorder the names on a line of `language_conflicts.txt` and
the other language takes the extension, or use `--force-language m=objective-c` for one run, one
project or one folder.

## Files that carry an extension without holding code

A `.pro` in an Android project is ProGuard rules, and a `.d` in a build directory is a make or cargo
dependency file. Counted as their extension they are Prolog and D, and the error grows with the size
of the build directory. mezura looks in the head of the file for the give-away lines listed in
`language_conflicts.txt`, and leaves such a file out.

`--show-skipped` prints the paths, `--explain` says which check set a file aside,
`--count-not-code` counts them anyway, and `--no-heuristics` turns the check off.

## Not counted at all

JSON, YAML, TOML, XML, CSV, Markdown and plain text ship with no language file. They are not code.
Jupyter notebooks are not counted either: a `.ipynb` is JSON with the source in arrays of strings,
which needs a reader and not a language file.

Adding a language of your own is one text file, and
**[the language files guide](LANGUAGE_FILES_GUIDE.md)** has a whole one to copy.
