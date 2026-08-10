# Cases still without a file

Files 01 to 49 are written and verified. What is listed here has no file yet, and each line says
why. When one lands, its line leaves this document.

## Waiting on a decision about the format

- **A file holding more than one language**, which is where the biggest miscount of all lives: a
  `.vue` counted whole with Vue's symbols, a `.html` whose `<script>` block uses `//` comments that
  are read as markup, a `.php` file holding HTML and JavaScript. tokei reports these as several
  languages and mezura reports one. Before any of them can be a case, the header has to be able to
  say which languages the file holds and what each one's share is, and that has not been designed.

## Waiting on parser work

- **C++ raw strings**, `R"( ... )"`. Undeclared because the closer would put `)` among the bytes the
  scan searches, which is a second pass over every line of C++. The delimited form
  `R"delim( ... )delim"` is variadic and out of reach whatever is decided.
- **Lua's `[[ ]]` string form.** The comment half of the long brackets is implemented and the string
  half is not, so a `[[ ]]` string body counts as code line by line.
- **Keywords are counted as words and not as meaning**, so a language that uses one of its keywords
  for a second purpose counts those too. A case would only document it; the fix is a lexer per
  language, which is not happening.

## Written but only half solved

- **The heredoc**, case 31. An apostrophe in the body opens a string and the comments under it are
  counted as code. A heredoc needs a delimiter rule of its own: an opener that names its own closer
  at runtime, which the format cannot express today.
- **The regex holding a comment opener**, case 37. Telling a regex from a division needs the token
  before the slash, which is a lexer's job.

## Definitions rather than mistakes

Cases 23, 24 and 47 hold the three places where mezura's numbers differ from a region-based counter
by design: a blank line inside a block comment, a line holding only punctuation, and a blank line
inside a multi line string. They pass as they are and are here as a reminder that when another
tool's pair of numbers is added, those three are where the pairs will differ without either tool
being wrong. The third is the largest of them by far on real trees: it is the whole of the Python,
Shell and Perl disagreement over the Linux kernel.
