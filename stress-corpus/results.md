# What the three counters answer

Every case in this folder was run through mezura, tokei 14.0.0 and scc 3.7.0. This document records
where the other two are wrong, and for scc it records why, since scc has no `scc-real` line inside
the case files to argue with.

**Which build of somebody else's counter a number came from is part of the number.** Both projects
release rarely and commit often: scc's last release is five months behind its main branch and
tokei's is eight, so "tokei" and "scc" name a moving target unless a build is written down beside
the figure. Neither build is the generous one on both axes, which is why the rule is per axis and
not per tool.

**A fault is reported only when both the release and the main branch have it.** Whichever of the two
is right wins, so nothing already fixed upstream is published as though somebody were still living
with it. This is the axis where main protects them and it costs one extra pass over the findings,
not over the corpus.

**A speed figure is measured on the released binary they distribute.** It is the only build they have
declared ready, and it is the one whose compiler flags are theirs rather than whatever a local
checkout happens to carry, which on this project alone was worth several percent. Where their main
branch is materially faster, that goes in a footnote with its commit. Performance does not accumulate
the way fixes do, so measuring an arbitrary commit can catch a project mid-refactor, and being caught
mid-refactor is not a result.

The two halves may therefore come from different builds, deliberately, and each figure says which.

**Not done yet.** This sweep is one build per tool: tokei six commits past its v14.0.0 tag and scc's
3.7.0 release binary. The findings have not been checked against either main branch.

**Every finding here was measured with the binaries.** The scc source read alongside them is a newer
checkout than the binary that produced the numbers, which is visible in its `--report` flag being
absent from 3.7.0, so nothing below rests on the source alone: each mechanism was confirmed by
running a file written to isolate it.

## Reading a disagreement before calling it a mistake

The three tools do not share a definition of what a line is, so a difference in a number is not by
itself a fault. Three definitions have to be settled first, and only what survives them is an error.

**mezura sorts a line into four buckets, the other two into three.** Ours are code, comment, extra
and blank; theirs are code, comment and blank. Their blank is a line of whitespace and nothing else,
so a line holding only punctuation, a `}` or a `]]` or a `?>`, has nowhere to go but code. In scc
this is not a judgement about braces, it is the fallback of the state machine: in `blankState` the
`default` arm sets the state to code for any byte that does not begin a comment or a string. So
**scc's code count is ours plus every punctuation-only line**, and that difference appears in most
cases in this folder without either answer being wrong. Cases 2400 and 6300 exist to show it.

**scc counts a Python docstring as a comment.** Its language file marks `"""` and `'''` with
`docString: true`, and a line that ends inside one is counted as comment rather than code. mezura and
tokei both call it a string, so all three of its lines are code. Case 2500 is three lines apart for
this reason alone. This is a definition and not a fault: a docstring usually is documentation.

**A blank line inside a block comment or a multiline string belongs to different buckets.** We call
it extra, they call it comment and code respectively. Cases 2300 and 4800 hold it.

What is left after those three is a genuine misreading, and that is what the rest of this document is.

## Where scc is wrong

**1. Haskell block comments do not nest.** `{- outer {- inner -} still outer -}` is one comment in
Haskell, and scc ends it at the inner `-}`, so the words after it count as code. Its language file
carries a `nested` flag and Haskell does not have it set. Case 1000, one line moved. The same shape
would hit any language whose comments nest.

**2. A quote inside a Rust character literal opens a string.** `let quote: char = '"';` leaves scc
inside a string that never ends, so both comments below it count as code. Its Rust definition
declares the double quote and every raw form and no apostrophe at all, so the character literal is
invisible to it. Case 1800, two lines moved. tokei is wrong here in exactly the same way.

**3. A raw string ending in a backslash never closes.** `r#"C:\ends\in\backslash\"#` is a complete
Rust string, and scc reads the backslash as escaping the closing quote, so everything below it is
counted as string content, which it counts as code. Its language file marks that form
`ignoreEscape: true`, which is meant to prevent exactly this, and the flag does not reach the case.
Measured on a two line file that differs only in the final backslash: without it the string closes,
with it the rest of the file is swallowed. Case 1600.

**4. The C++ raw string `R"( ... )"` is declared and not honoured.** Its language file lists the form
and its four prefixed spellings, and the counting behaves as though none of them were there: on
`R"(has a " lone quote)"` the lone quote inside pairs with the opening one, which is only possible if
the plain double quote matched and the raw opener did not. Case 6500, four lines moved, and scc lands
on the same answer a counter with no raw strings at all would give. This one is a measurement: the
behaviour reproduces on a two line file, and reading the trie did not explain it, so the cause is
recorded as unknown rather than guessed at.

**5. A CMake line beginning `#[` loses its comment entirely.** This is the widest of them. scc
declares `#` as CMake's line comment and `#[[` as its bracket comment, and its matcher walks the
symbol table one byte at a time, remembering the last place a **string** opener was found and nothing
else. On `#[==[` it walks `#`, then `[`, then fails on `=`, and returns no match at all rather than
falling back to the `#` it already passed. The line counts as code. Verified on the smallest possible
input: `# a comment` is a comment, `#[[ a comment` is a comment, and both `#[ a comment` and
`#[x a comment` are code. Case 4400, and any CMake file that writes `#[` before anything but a second
bracket.

**6. Sections of another language are not counted at all.** scc has no notion of a file holding more
than one language: `children` appears nowhere in its language file and nothing in its processor looks
for it. So a `//` comment inside a `<script>` block is read with the page's symbols, which have no
`//`, and counts as markup code. This costs it cases 5100, 5300, 5500, 6200, 6600 and 6900, which is
every case in the folder about embedded languages. tokei does have the feature and gets most of them.

**7. Wrong in company.** The heredoc holding an apostrophe (3100), the regex holding a comment opener
(3700), C++'s delimited `R"delim( ... )delim"` (6700), Lua's `[[ ]]` string (6800) and the SQL path
ending in a backslash (7000) defeat all three counters, mezura included. They are listed here so the
count of scc's faults is not read as a score.

**A backslash escapes in every language, and in several it means nothing.** Case 7000 is the sharpest
of them and all three tools fail it identically. Standard SQL escapes a quote by doubling it, `''`,
and the backslash is an ordinary character, so `path = 'C:\'` is a closed string holding a Windows
path. All three read the backslash as cancelling the closing quote, and since all three let SQL
strings cross lines, every comment below is counted as string content: 3 code and 7 comment where the
honest answer is 2 and 8, and on a real file the loss runs to the end of it.

The same hardcoded backslash sits over a whole family: Pascal, Delphi, Ada, Fortran and COBOL all
escape by doubling too, and all five are shipped here. They escape lightly because their quotes are
declared as ending with their line, so the damage stops at that line and never changes a count, only
which words on it are searched for keywords. SQL is the one where the quote crosses lines, and that
is why it is the case that got written.

## Where scc is right and we are not

**The escaped apostrophe in unquoted shell text**, case 6000. `echo I\'m done` writes one apostrophe
and opens nothing. We treat the single quote as a form that escapes nothing, apply that to the
opening symbol as well as the closing one, and count the comments below as code. scc and tokei both
read it correctly.

**A comment opener inside an HTML attribute**, case 4000. We declare no string symbols for HTML on
purpose, so a `<!--` written inside a quoted attribute value opens a comment that never closes. Both
of the others get it right.

## Where scc is right and tokei is not

scc answers correctly, and tokei does not, in cases 1300, 1400, 1900, 3900, 4600, 4700, 4900, 5000,
5400 and 6400: Lua's counted brackets, a string opening after a block closes, an apostrophe in HTML
text, a block comment closed by `//*/`, code between two comments on one line, a doc comment with no
text, and a statement after a block closes. In case 4600 scc is the only one of the three to close
the block at `//*/`, which is the C idiom for switching a block off by adding one character.

## Where tokei is wrong

tokei misreads 29 of the 70 cases. Each of those files carries a `tokei:` line saying what it gets
wrong, so the detail is next to the evidence rather than repeated here. By mechanism:

**A quote it should not have seen opens a string, and everything below counts as code.** Cases 1800,
3000, 3100, 3900, 5100 and 5700. The apostrophe of "it's", a quote inside a regex, a quote inside a
character literal, a backslash before a closing quote.

**A comment ends in the wrong place.** Cases 1300, 1400, 4400 and 5900: a bare `]]` ends a counted
bracket that only an end of the same count should close, and an inner closer ends a block that nests.

**A comment does not end at all, or ends and the code after it is lost.** Cases 1900, 2200, 4600,
4700, 6400.

**A doc comment is counted as neither comment nor blank.** Cases 4900 and 5000, where a file of 12
lines is reported as 11.

**A section is not found or is misnamed.** Cases 5200, 5300, 5400, 5500, 6200, 6600, 6900. Its
TypeScript sections come out labelled JavaScript, and a tag it does not recognise takes its whole
section with it.

**Wrong in company**, as above: 2900, 3700, 6700, 6800.

## The numbers

Written as `lines / code / comment`, as each tool reports today.

| case | mezura | tokei | scc |
|---|---|---|---|
| 0100 escaped quote before comment | 7/2/5 | 7/2/5 | 7/2/5 |
| 0200 comment symbol inside string | 7/2/5 | 7/2/5 | 7/2/5 |
| 0300 string symbol inside comment | 7/1/6 | 7/1/6 | 7/1/6 |
| 0400 block comment holding line comment | 9/1/8 | 9/1/8 | 9/1/8 |
| 0500 line comment holding block open | 7/1/6 | 7/1/6 | 7/1/6 |
| 0600 code then comment on one line | 7/2/5 | 7/2/5 | 7/2/5 |
| 0700 close touching reopen plain | 9/1/8 | 9/1/8 | 9/1/8 |
| 0800 close touching reopen nesting | 9/1/8 | 9/1/8 | 9/1/8 |
| 0900 close and reopen with a space | 9/1/8 | 9/1/8 | 9/1/8 |
| 1000 nested block comment that nests | 7/1/6 | 7/1/6 | **7/2/5** |
| 1100 nested block comment that does not | 7/1/6 | 7/1/6 | 7/1/6 |
| 1200 two comment pairs of one language | 8/1/7 | 8/1/7 | 8/1/7 |
| 1300 lua level bracket holding plain close | 10/1/9 | **10/3/7** | 10/1/9 |
| 1400 lua short closer hiding the real one | 9/1/8 | **9/2/7** | 9/1/8 |
| 1500 raw string holding quotes | 7/1/6 | 7/1/6 | 7/1/6 |
| 1600 raw string ending in backslash | 7/1/6 | 7/1/6 | **7/2/5** |
| 1700 verbatim string crossing lines | 8/2/6 | 8/2/6 | 8/2/6 |
| 1800 char literal holding a quote | 9/1/8 | **9/3/6** | **9/3/6** |
| 1900 string opening after block close | 9/1/8 | **9/0/9** | 9/1/8 |
| 2000 lifetime and apostrophe in string | 8/1/7 | 8/1/7 | 8/1/7 |
| 2100 line splice inside a string | 9/4/5 | 9/4/5 | 9/4/5 |
| 2200 line splice inside a comment | 9/1/8 | **9/2/7** | **9/2/7** |
| 2300 blank line inside block comment | 10/1/8 | 10/1/9 | 10/1/9 |
| 2400 punctuation only line | 9/2/6 | 9/3/6 | 9/3/6 |
| 2500 docstring holding a comment symbol | 10/5/5 | 10/5/5 | 10/2/8 |
| 2600 backtick template holding a comment | 9/4/5 | 9/4/5 | 9/4/5 |
| 2700 string symbol of another kind inside | 7/2/5 | 7/2/5 | 7/2/5 |
| 2800 comment pair opening inside a string | 8/2/6 | 8/2/6 | 8/2/6 |
| 2900 two char literals holding escapes | 13/2/10 | **13/6/7** | **13/6/7** |
| 3000 regex literal holding a quote | 10/2/8 | **10/3/7** | **10/3/7** |
| 3100 heredoc holding an apostrophe | **13/5/8** | **13/5/8** | **13/5/8** |
| 3200 odd triple quote count | 11/4/7 | 11/4/7 | 11/4/7 |
| 3300 primed identifier and apostrophe | 8/2/6 | 8/2/6 | 8/2/6 |
| 3400 transpose operator and char | 8/2/6 | 8/2/6 | 8/2/6 |
| 3500 primed identifier haskell | 8/2/6 | 8/2/6 | 8/2/6 |
| 3600 primed identifier ocaml | 8/2/6 | 8/2/6 | 8/2/6 |
| 3700 regex holding a comment opener | **11/1/10** | **11/1/10** | **11/1/10** |
| 3800 character written with a backslash | 8/2/6 | 8/2/6 | 8/2/6 |
| 3900 html has no strings | 9/2/7 | **9/3/6** | 9/2/7 |
| 4000 comment opener inside an attribute | **11/1/10** | 11/3/8 | 11/3/8 |
| 4100 closer in code with nothing open | 7/1/6 | 7/1/6 | 7/1/6 |
| 4200 three slashes are one comment | 7/1/6 | 7/1/6 | 7/1/6 |
| 4300 pascal writes two pairs | 8/1/7 | 8/1/7 | 8/1/7 |
| 4400 cmake bracket comment | 10/1/9 | **10/3/7** | **10/4/6** |
| 4500 multiline string ending in backslash | 12/2/10 | 12/2/10 | 12/2/10 |
| 4600 block comment closed after a line comment | 14/2/10 | **14/2/12** | 14/4/10 |
| 4700 comment then code then comment | 9/2/7 | **9/1/8** | 9/2/7 |
| 4800 blank line inside a multiline string | 12/5/6 | 12/6/6 | 12/6/6 |
| 4900 doc comment with no text | 13/1/12 | **13/1/11** | 13/1/12 |
| 5000 doc comment with no text ending a block | 12/1/11 | **11/1/10** | 12/1/11 |
| 5100 vue sections count as their own languages | 27/9/17 | **27/10/16** | **27/13/14** |
| 5200 section language named by the tag | 26/9/17 | 26/9/17 | 26/9/17 |
| 5300 script tag in upper case | 15/5/10 | **15/6/9** | **15/6/9** |
| 5400 script closer inside a string | 17/5/12 | **18/6/12** | 17/5/12 |
| 5500 section named by a mime type | 15/3/12 | 15/3/12 | **15/4/11** |
| 5600 verbatim string ending in a backslash | 9/2/7 | 9/2/7 | 9/2/7 |
| 5700 wysiwyg string ending in a backslash | 11/2/9 | **11/3/8** | **11/3/8** |
| 5800 here string holding an apostrophe | 11/4/7 | 11/4/7 | 11/4/7 |
| 5900 nested block comment in odin | 13/2/11 | **13/4/9** | **13/4/9** |
| 6000 escaped apostrophe outside quotes | **13/4/9** | 13/2/11 | 13/2/11 |
| 6100 script opener that never closes | 9/2/7 | 9/2/7 | 9/2/7 |
| 6200 script closer written with a space | 14/4/10 | 14/4/10 | 14/4/10 |
| 6300 brace and trailing comment | 8/2/6 | 8/3/5 | 8/3/5 |
| 6400 close then code then stray and reopen | 10/2/8 | **10/1/9** | 10/2/8 |
| 6500 raw string holding its own closing bracket | 11/5/6 | 11/5/6 | **11/1/10** |
| 6600 opening tag split over two lines | 13/5/8 | 13/5/8 | 13/5/8 |
| 6700 delimited raw string names its own closer | **11/1/10** | **11/1/10** | **11/1/10** |
| 6800 long bracket string holding a comment symbol | **10/1/8** | **10/2/8** | **10/2/8** |
| 6900 php block holding html around it | **15/5/8** | **15/7/8** | **15/7/8** |
| 7000 backslash before a closing quote in sql | **10/3/7** | **10/3/7** | **10/3/7** |

Bold marks an answer that is wrong under that tool's own definitions. An unbolded difference is one
of the three definitions above. Cases 6200 and 6600 are bold nowhere and wrong in all three: the
totals agree while the section is lost, which is what the `real-section` lines are for.

## How this was measured

```
mezura.exe <case> --hide version,timing
tokei.exe <case>
scc.exe <case>
```

Where a tool disagreed with either of the other two, the case was counted by hand. Where scc's answer
needed explaining, `scc.exe -t <case>` prints its verdict for every line, which says which line moved,
and the mechanism was then confirmed on a file written to isolate it rather than inferred from the
source. Findings 1, 2, 3, 5 and 6 were confirmed that way. Finding 4 is the measurement only.
