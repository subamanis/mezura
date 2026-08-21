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
| `Shebangs` *(opt)* | Interpreters a `#!` first line may name, for scripts with no extension | `sh bash zsh` |
| `String symbols` | Strings that end with the line | `" '` |
| `Character literal symbols` *(opt)* | Wraps a single character, like Rust's `'a'` | `'` |
| `Multi line string symbols` *(opt)* | Crosses lines, backslash escapes | `"""` |
| `Multi line raw string symbols` *(opt)* | Crosses lines, nothing escapes | `` ` `` |
| `Paired string openers` *(opt)* | Opens with one symbol, closes with another | `r#" @"` |
| `Paired string closers` | Their closers, in the same order | `"# "` |
| `Line continuation` *(opt)* | Joins a line to the next when it ends the line | `\` |
| `Continues` | What the joining reaches: `strings`, `comments`, or both | `strings comments` |
| `Comment symbols` | Comments that end with the line | `// #` |
| `Multi line comment start` *(opt)* | Block comment openers | `/* {` |
| `Multi line comment end` | Their closers, in the same order | `*/ }` |
| `Self-nesting comment start` *(opt)* | Openers of blocks that nest inside themselves | `(*` |
| `Self-nesting comment end` | Their closers, in the same order | `*)` |
| `Nested language start` *(opt)* | Openers of sections written in another language | `<script <style` |
| `Nested language end` | Their closers, in the same order | `</script> </style>` |
| `Nested language default` | The extension each section falls to when its tag names none | `js css` |
| `Keyword` *(opt, repeatable)* | What to count beside the lines | see below |

A block marked *(opt)* can be left out entirely. One that has "in the same order" under it comes
with its partner or not at all.

`Shebangs` is consulted only for a file with no extension whose name nothing claims: its first
line is read, and the interpreter named there, found past `/usr/bin/env` and its flags, is matched
against these names. A versioned interpreter falls back to its plain name, so `python` alone
covers `python3` and `python3.12`; name a versioned form explicitly only when it belongs to a
different language, the way `perl6` is Raku and not Perl.

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
escaping in Java, Swift and Python. Look it up rather than guessing, because getting it wrong is
silent: a `` `C:\` `` in the wrong block leaves the string open to the end of the file and every
comment under it counts as code.

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
work. The extension comes first because that is the form your `extension_priority.txt` answers for,
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
`extension_priority.txt` or the one `--force-language` gave, so one word never selects one language
and counts another.

Because that answer is your machine's, a configuration file you share is clearer if it names
languages by their names. On the command line, type whichever is shorter.

## Two things that bite

**A `'` in the wrong block.** In a language with character literals, declaring `'` under
`String symbols` works until the first apostrophe in an English word inside a comment. Use
`Character literal symbols` and a lone `'` opens nothing, which is also what keeps Rust lifetimes
like `&'a str` from swallowing the line.

**Two languages wanting the same extension.** Only one can have it, and the loser's files are then
read with the winner's symbols. Name the winner in `extension_priority.txt` in the data directory,
under `contested-extensions` or `contested-filenames`, or use `--force-language` for one run.

## Checking it

Run mezura over a folder holding one file of your language. If the file could not be read, mezura
says so at the top of the run and names the line.
