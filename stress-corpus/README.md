# A corpus of line-counting traps

Small source files, one trap each, for anyone testing a line counter. They are numbered roughly
from ordinary to deranged, and the language of each file is whichever one the trap is natural in,
so the folder is mixed on purpose.

Every file is meant to be counted **on its own**. One trap per file is the rule, so that a wrong
answer on one case cannot move the numbers of another, and so any tool can be pointed at exactly
one case.

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
hand. Two tools can therefore carry different `real` numbers for one file and both be right.
**`<tool>-count`** is what the tool actually prints today. A tool passes a case when its two numbers
agree and fails it when they do not, so a tool is only ever judged against what it says it wants,
never against another tool's idea of correctness.

**`trap:`** says what the file is about, in one or two lines, and belongs to no tool.

**`<tool>:`** is an optional note explaining why that tool's answer differs from its own intent. It
appears only where the pair above disagrees, and only for the tool it names.

There is no "known wrong" marker, because there is nothing left for it to mean: the two numbers say
it, and they say it per tool. The counts include the header lines themselves, since a tool counting
the file sees them.

## Reading the numbers across tools

Counters do not agree on definitions, and most disagreements are not bugs. One asks what a line
says, so a blank line inside a block comment is blank and a line holding only `}` is neither code
nor comment. Another asks which block a line sits in, and gives that blank line to the comment and
the brace to the code. Both are defensible. Cases 23 and 24 are exactly those two places, and they
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
