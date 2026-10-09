# Adding a language

A language is one plain text file in the data directory the program made on its first run:

```
Windows:  %APPDATA%\mezura\data\languages\
Linux:    ~/.local/share/mezura/languages/
macOS:    ~/Library/Application Support/mezura/languages/
```

The quickest way in is to copy the file of a language that looks like yours and edit it. This is Go,
complete, and most files are about this size:

```
Language
Go

Extensions
go

String symbols
"

Multi line raw string symbols
`

Comment symbols
//
Multi line comment start
/*
Multi line comment end
*/

Keyword
NAME
structs
ALIASES
struct
```

Three things to know before you start:

- A header is one line and its value is the **next** line, always. Blank lines between blocks are
  free, but never between a header and its value.
- Some values are meant to be empty, and then the empty line is the answer. HTML has `String symbols`
  with nothing under it because its quotes are not strings.
- **The blocks must come in the order of the table below.** One out of place, or one header
  misspelt, and the file is refused whole and the language is left out of the run. Mezura tells you
  the line it stopped at.

## The blocks

| Block | What it is | Example value |
|---|---|---|
| `Language` | The name shown in the report | `Kotlin` |
| `Extensions` | Extensions, no dot, case ignored | `cpp cxx cc` |
| `Filenames` *(opt)* | Whole names, for files an extension cannot describe | `Makefile Dockerfile` |
| `Shebangs` *(opt)* | Interpreters a `#!` first line may name, for scripts with no extension and for a contested one | `sh bash zsh` |
| `Identifying line starts` *(opt)* | Comma-separated literals; a line beginning with one, blanks aside, identifies a contested file as this language | `function, classdef, %` |
| `Identifying line contains` *(opt)* | The same, found anywhere in a line | `std::` |
| `String symbols` | Strings that end with the line | `" '` |
| `Character literal symbols` *(opt)* | Wraps a single character, like Rust's `'a'` | `'` |
| `Multi line string symbols` *(opt)* | Crosses lines, backslash escapes | `"""` |
| `Multi line raw string symbols` *(opt)* | Crosses lines, nothing escapes | `` ` `` |
| `Paired string openers` *(opt)* | Opens with one symbol, closes with another | `r#" @"` |
| `Paired string closers` | Their closers, in the same order | `"# "` |
| `Paired strings escaped by doubling` *(opt)* | The openers of the pairs whose closer written twice is one quote of the text | `@"` |
| `Escape character` | Required by any language that declares a string. See below | `\` |
| `Line continuation` *(opt)* | Joins a line to the next when it ends the line | `\` |
| `Continues` | What the joining reaches: `strings`, `comments`, or both | `strings comments` |
| `Comment symbols` | Comments that end with the line | `// #` |
| `Multi line comment start` *(opt)* | Block comment openers | `/* {` |
| `Multi line comment end` | Their closers, in the same order | `*/ }` |
| `Self-nesting comment start` *(opt)* | Openers of blocks that nest inside themselves | `(*` |
| `Self-nesting comment end` | Their closers, in the same order | `*)` |
| `Cancelled symbols` *(opt)* | Symbols that stop counting when one character sits in front of them | `<* *>` |
| `Cancelled after` | The character that cancels each, in the same order | `[ <` |
| `Nested language start` *(opt)* | Openers of sections written in another language | `<script <style` |
| `Nested language end` | Their closers, in the same order | `</script> </style>` |
| `Nested language default` | The extension each section falls to when its tag names none | `js css` |
| `Tests` *(opt)* | What a test starts with, what makes a whole file one, and which file names are test files | see below |
| `Keyword` *(opt, repeatable)* | What to count beside the lines | see below |

A block marked *(opt)* can be left out entirely. One that has "in the same order" under it comes
with its partner or not at all.

`Shebangs` is consulted for a file with no extension whose name nothing claims, and for a file whose
extension two languages claim: its first line is read, and the program named there, found past
`/usr/bin/env` and its flags, is matched against these names. A versioned interpreter falls back to
its plain name, so `python` alone covers `python3` and `python3.12`; name a versioned form
explicitly only when it belongs to a different language, the way `perl6` is Raku and not Perl.
`--no-shebang` reads no such line, so those extensionless files go uncounted and a contested
extension is settled without it.

## Which string block

Two questions decide it:

**Does it end with the line, or can it run over several?**

**Does a backslash before the closer cancel it?** In Java `"a\"b"` is one string. In Go `` `a\` `` is
a whole string ending in a backslash, because a backtick string escapes nothing.

| Your string | Block |
|---|---|
| Ends with the line | `String symbols` |
| One character, `'a'` | `Character literal symbols` |
| Crosses lines, backslash escapes | `Multi line string symbols` |
| Crosses lines, nothing escapes | `Multi line raw string symbols` |
| Different symbol at each end | `Paired string openers` + `closers` |

**A symbol goes in exactly one of them.** Declaring it twice refuses the file.

Raw or not is a fact about the language, not about the symbol. The backtick escapes nothing in Go,
Odin and D and does escape in a JavaScript template literal. `"""` is raw in Kotlin and Scala,
escaping in Java, Swift and Python. A raw form whose closer is one character written three times
or more ends at the last of a run of them, since nothing escapes a quote in front of it: Kotlin's
`"""say "hi""""` holds `say "hi"`. Look it up rather than guessing, because getting it wrong is
silent: a `` `C:\` `` in the wrong block leaves the string open to the end of the file and every
comment under it counts as code.

## The escape character

Whatever declares a string of any kind has to say what cancels a quote, in an `Escape character`
block after the string blocks and before `Line continuation`. **Leave it out and the file is
refused**, which is the one way a file that looks complete is not.

The value is one ASCII character, or the word `none`. It is the backslash in most languages, the
backtick in PowerShell, and `none` in the family that escapes a quote by doubling it: Pascal,
Delphi, Ada, Fortran, COBOL, MATLAB, VHDL and SQL. Getting it wrong is as silent as getting a
string block wrong: an SQL statement holding `'C:\'` swallows every comment under it to the end of
the file when the backslash is declared as an escape and SQL has none.

This says which character escapes, not where it is obeyed. Whether it works inside one form that
crosses lines is decided by the block that form sits in, above. A language with no strings at all,
like HTML, can leave the block out.

A pair has no escape character inside it, and some write their closer twice for one quote of the
text: `""` inside C#'s and F#'s `@"..."`. The openers of those pairs go under `Paired strings
escaped by doubling`, right after the closers, and naming an opener no pair declares refuses the
file. A string that opens and closes with the same symbol needs nothing of the kind, since
`'it''s'` read as two strings side by side is the same reading.

## Which comment block

`Multi line` if the first closer ends the block, the way `/* */` works in C. `Nesting` if the block
ends only after as many closers as openers, the way `(* *)` works in OCaml and `/* */` in Rust.

A language can declare several pairs, matched by position, and only the closer of the pair that
opened a block ends it. Pascal writes both `{ }` and `(* *)`, so a stray `*)` inside a `{ }` comment
is text.

The two characters `=*` mean "any number of `=` here", which declares Lua's `--[[ ]]`, `--[=[ ]=]`
and every level above in one line:

```
Multi line comment start
--[=*[
Multi line comment end
]=*]
```

That is the only place in the format where characters do not stand for themselves, and it works
only in these two blocks.

## When a symbol is part of something longer

A few languages write one of their symbols inside a longer form of their own, where it is not that
symbol at all. C3 opens a documentation comment with `<*` and closes it with `*>`, and writes a
vector of unknown length as `int[<*>]`, where `[<` is one token and neither half is a comment:

```
Cancelled symbols
<* *>
Cancelled after
[ <
```

Read it as "a `<*` right after a `[` is not a comment opener, and a `*>` right after a `<` is not a
closer". The two lines are matched by position, the symbol has to be one this file declares
somewhere, and what cancels it is one character sitting immediately in front of it. A symbol at the
very start of a line has nothing in front of it and always counts.

## Sections of another language

Some files are a shell holding blocks of other languages: a page with `<script>` and `<style>`, a
Vue or Svelte component. Declare those blocks and each one is counted with its own language's
symbols, so a `//` inside a script block is a comment even though the shell has no `//` at all.

The three lines are matched by position, one entry per kind of block:

```
Nested language start
<script <style
Nested language end
</script> </style>
Nested language default
js css
```

**`Nested language default` is the language the block falls to when its opening tag names none.**
`<script>` on its own is JavaScript, `<script lang="ts">` is TypeScript, `<style>` is CSS,
`<style lang="scss">` is SCSS.

**How a spelling becomes a language, in both the tag and the default: extension first, then the
language's own name.** So `ts` is found because TypeScript claims that extension, and `typescript`
is found because that is what the language is called, which is what makes `type="text/typescript"`
work. The extension comes first because that is the form your `language_conflicts.txt` answers for,
so an extension two languages claim resolves to the same one the counting uses. A spelling nobody
recognises falls to the region's default rather than losing the block.

The opener reads `lang="..."` first and `type="..."` after it, and in a mime type only the part
after the slash is the language. Quotes are optional and either kind works.

**Every opener and closer has to begin with `<`**, because a section is looked for where a tag
begins and nowhere else. A file declaring anything else is refused and mezura names the line, rather
than accepting a declaration that could never match. The name of the tag also has to end where it is
written, so `<script` opens a section in `<script>` and `<script lang="ts">` and not in
`<scriptures>`.

**A section that never closes is not a section.** Nothing marks an opener as a tag rather than the
same word written out in the text of the page, so if no closer follows, the lines stay with the file's
own language instead of being handed to another one to the end of the file.

**A whole shell language** looks like this, and Vue's real file is barely longer:

```
Language
Vue

Extensions
vue

String symbols

Comment symbols

Multi line comment start
<!--
Multi line comment end
-->

Nested language start
<script <style
Nested language end
</script> </style>
Nested language default
js css
```

Note what it does **not** declare: no string symbols, because the quotes of markup delimit
attributes and its text is full of apostrophes, and no `//`, because the shell has no such comment.
Everything that needs those lives inside the blocks and carries its own language's rules.

## Test code

Some languages put tests in the same file as the code. Declare what a test starts with, and the
lines from there to the end of that item go to a `tests` row under the language. Where a test
runner collects tests by an annotation, declare the annotation, and every line of a file holding
one goes to the same row. Where the toolchain itself gives test files a name, declare the name,
and every line of those files goes there too.

```
Tests
    MARKERS
    #[ #![
    MODULES
    mod
```

```
Tests
    FILE MARKERS
    [Test [TestFixture [Fact [Theory
```

```
Tests
    FILE NAMES
    *_test.go
```

Any of the four can be left out, in the order `MARKERS`, `MODULES`, `FILE MARKERS`, `FILE NAMES`
when more than one is there. A block with none refuses the file, and so does `MODULES` without
`MARKERS`, since a declaration counts only on a line a marker opened. `MODULES` and `FILE MARKERS`
do not go together either, since no rule says what a file an annotation made test code declares.

`MARKERS` is what is searched for. `#[` and `#![` are read as Rust attributes: any attribute with
`test` in its name starts a test, `#[cfg(...)]` starts one when its predicate has `test` outside a
`not(...)`, and `#![cfg(test)]` makes the rest of its scope tests, the whole file at the top of one
and the module it sits in. Any other marker is a plain word, like D's `unittest`, and one followed
by `):`, D's `version(unittest):`, covers the rest of its scope the same way. A test runs from the
marker to the brace matching the first `{`, or to the first `;`, whichever comes first, and an
`if` continues through its `else`.

`MODULES` is the one word that declares a module living in another file, `mod` for Rust. Such a
declaration on a line of test code, `#[cfg(test)] mod tests;`, makes the file it names test code
whole, and so every file that one declares in turn. The file is found the way rustc finds it:
`x.rs` or `x/mod.rs` beside a `lib.rs`, `main.rs` or `mod.rs`, under a folder named after any
other file, and beside it for a crate root, with an inline `mod outer { }` adding `outer/`. A
declaration under `#[path = "..."]` is not followed.

`FILE MARKERS` are the annotations a test runner collects tests by, `@Test` for Java, `[Fact` for
C#, and the first one found in the code of a file makes the whole file test code: the class around
a test method is the test class. A marker is matched as written and has to stand as a word of its
own, so the byte before it and the byte after it may be no letter, digit or underscore, and the
byte after it may not be a `.` either. That is why C#'s are written with the bracket open: `[Test`
matches `[Test]`, `[Test, Order(1)]` and `[Test(Description = "x")]`, and refuses `[TestFixture]`,
which gets a line of its own, an indexer `map[Test]`, and a collection expression `[Test.Of(a)]`.
A marker that starts with `[` counts only where nothing but other bracketed lists stands in front
of it on its line, which is where an attribute sits, so `grid[0][Test]`, `args is [Test]` and
`{ [Test] = 1 }` mark nothing. A marker inside a string or a comment is text. Beside `MARKERS`, a file marker inside the test code
one of them opened belongs to that test and leaves the rest of the file as it was, since an
annotation inside a test block says nothing about the code around it. List only what a test
framework defines and no production library writes: `@Before` is AspectJ's as much as JUnit 4's,
so it is not on Java's list. A file a `!` pattern of `--tests` took back is not read for these,
since the `!` answered the question they answer.

`FILE NAMES` takes three shapes only: `*suffix`, `prefix*` and a whole name, case-sensitive. It is
for a name the toolchain defines, the way the go tool builds `*_test.go` only for `go test` and a
Perl `.t` file is a test script and nothing else. A name a framework or a team merely favours is
declared by whoever counts, with `--tests`. The directory a build tool compiles for tests alone,
`tests` beside a `Cargo.toml` or `src/test` beside a `pom.xml`, is built in and the same for every
language, so file names are only checked outside one.

What each language gets right and what it misses is in `TEST_DETECTION.md`.

## Keywords

`NAME` is what appears in the report, `ALIASES` are the words in the code that count as one. Add as
many `Keyword` blocks as you like.

```
Keyword
NAME
classes
ALIASES
class record
```

They are counted as plain words: a word in a string or a comment never counts, `aclass` is not a
`class`, and a language that uses `class` for a second purpose has those counted too.

## Naming a language on the command line

Wherever a language name is expected, `--languages`, `--exclude-languages`, and the same fields in
a configuration file, you can write **either the name the file gives it or any extension it
claims**: `--languages javascript` and `--languages js` are the same request. An extension that two
languages claim names whichever of them owns it for the counting, which is the answer in
`language_conflicts.txt` or the one `--force-language` gave, so one word never selects one language
and counts another.

Because that answer is your machine's, a configuration file you share is clearer if it names
languages by their names. On the command line, type whichever is shorter.

## Two things that bite

**A `'` in the wrong block.** In a language with character literals, declaring `'` under
`String symbols` works until the first apostrophe in an English word inside a comment. Use
`Character literal symbols` and a lone `'` opens nothing, which is also what keeps Rust lifetimes
like `&'a str` from swallowing the line.

**Two languages wanting the same extension.** Only one can have it, and the loser's files are then
read with the winner's symbols. Name the winner in `language_conflicts.txt` in the data directory,
under `contested-extensions` or `contested-filenames`, or use `--force-language` for one run.
`--show-languages` stars every extension your language lost and names who took it.

## Checking it

Run mezura over a folder holding one file of your language. If the file could not be read, mezura
says so at the top of the run and names the line.

`--show-languages` is the other half of the check. Your language belongs on that list with the
extensions you gave it beside it, and a star on one of them means another language takes those
files.
