# A corpus of line-counting traps

Small source files, one trap each, for anyone testing a line counter. They are numbered roughly
from ordinary to deranged, and the language of each file is whichever one the trap is natural in,
so the folder is mixed on purpose.

Every file is meant to be counted **on its own**. One trap per file is the rule, so that a wrong
answer on one case cannot move the numbers of another, and so any tool can be pointed at exactly
one case.

They are numbered in hundreds, so that a new case can be given a number beside the one it belongs
next to instead of at the end, without every case after it having to move.

## What the header says

The first lines of each file are a comment in that file's own language:

```
// mezura-real  10 lines 3 code 7 comment
// mezura-count 10 lines 5 code 5 comment
// tokei-real   10 lines 4 code 7 comment
// tokei-count  10 lines 4 code 7 comment
// trap: an apostrophe inside a heredoc body
// mezura: the apostrophe opens a string and every comment under it is counted as code
```

**`<tool>-real`** is the right answer for that file *under that tool's own definitions*, counted by
hand. Two tools can therefore carry different `real` numbers for one file and both be right: one
asks what a line says and another asks which block it sits in, and a blank line inside a comment
belongs to neither answer more than the other. **`<tool>-count`** is what the tool actually prints
today. A tool passes a case when its two numbers agree and it found every section the file declares,
and fails it when either half is not, so a tool is judged against what it says it wants and never
against another tool's idea of correctness.

**`<tool>-section`** is for a file that holds sections of another language, a `<script>` block in a
page or the three parts of a Vue component. One line per section language, naming it and its counts:

```
// mezura-section  TypeScript 2 lines 1 code 1 comment
// mezura-section  SCSS       2 lines 1 code 1 comment
// tokei-section   JavaScript 2 lines 1 code 1 comment
// tokei-section   CSS        2 lines 1 code 1 comment
// required-section TypeScript 2 lines 1 code 1 comment
// required-section SCSS       2 lines 1 code 1 comment
```

Such a case needs them, because the three totals of the file are the same whether the sections were
found at all or the whole file was read with one language's symbols. Two tools can also agree on
every number and disagree on what the sections *are*, which is what these lines exist to record.

**`required-section`** carries no tool's name, and it is not the whole truth about the file. It is
the shorter, harder list: the sections the file itself declares, which every tool is obliged to
find. A block that names its language names it for everyone, so a `<script lang="ts">` holds
TypeScript and a tool calling it JavaScript has ignored what it was told. A block that names none
still declares one through its default, so a bare `<script>` in a page holds JavaScript.

**A tool is judged on that list and on nothing else about sections.** Every entry must appear among
its own `<tool>-section` lines with those counts; anything it reports beyond them is its own model
and is not held against it. That second half matters, because part of this really is taste. Whether
the markup inside a Vue `<template>` is HTML or is the Vue file itself has no right answer: its
syntax is HTML's, and what people write in it is `<MyComponent />`, `v-if` and `{{ }}`, which HTML
has none of. Whether the prose inside a Rust doc comment is Markdown is the same kind of question.
A counter that splits those out is not wrong, and neither is one that does not.

What a tool does owe, once it has decided, is consistency: a block it calls HTML has to be counted
with HTML's rules from then on. Getting that wrong is not a matter of taste and shows up where it
belongs, in the two totals.

**`trap:`** says what the file is about, in one or two lines, and belongs to no tool.

**`<tool>:`** is a note saying what that tool gets wrong here. It appears exactly where that tool's
answer is not the right one, whether the difference is in its totals or in its sections. The runner
enforces this for every tool by the same rule and in both directions: a case a tool gets wrong while
saying nothing reads as a passing case, and a note left behind after the wrong answer was fixed
reads as a fault that is no longer there. A case that declares nothing about a tool is simply not
measuring that tool, which is most of them.

There is no "known wrong" marker, because there is nothing left for it to mean: the two numbers say
it, and they say it per tool. The counts include the header lines themselves, since a tool counting
the file sees them.

## Reading the numbers across tools

Counters do not agree on definitions, and most disagreements are not bugs. One asks what a line
says, so a blank line inside a block comment is blank and a line holding only `}` is neither code
nor comment. Another asks which block a line sits in, and gives that blank line to the comment and
the brace to the code. Both are defensible. Cases 2300 and 2400 are exactly those two places, and they
exist so that the difference is visible rather than argued about.

## Running it

mezura asserts its own pair from its test suite, so a fix has to update the `mezura-count` line in
the same commit and a wrong answer that changes shape is caught rather than absorbed. Any other
tool can be run over the folder by hand today. A runner that takes several tools and prints one
comparison is the eventual shape of this and does not exist yet.

## Licence

CC0, in the `LICENSE` file beside this one, and it covers this directory only. Copy these files
into your own repository without asking and without attribution: they are test inputs, and that is
what they are for. The rest of the mezura repository is MIT or Apache-2.0.
