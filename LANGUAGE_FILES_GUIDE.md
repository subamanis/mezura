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
| `Nesting comment start` *(opt)* | Openers of blocks that nest inside themselves | `(*` |
| `Nesting comment end` | Their closers, in the same order | `*)` |
| `Keyword` *(opt, repeatable)* | What to count beside the lines | see below |

A block marked *(opt)* can be left out entirely. One that has "in the same order" under it comes
with its partner or not at all.

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
