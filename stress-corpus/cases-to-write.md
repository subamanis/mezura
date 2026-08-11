# Cases still without a file

Files 01 to 59 are written and verified. What is listed here has no file yet, and each line says
why. When one lands, its line leaves this document.

## Waiting on parser work

- **A file whose languages interleave rather than sit in blocks**: a `.php` holding HTML, and the
  ERB, JSP and Blade family. The switch happens anywhere, including in the middle of a line, and
  both ways, which is a different problem from the `<script>` and `<style>` blocks that cases 50
  to 54 cover. Nothing counts these correctly, tokei included, and the design chapter for it is
  the "interleaved foresight" section of `archive/EMBEDDED_LANGUAGES.md`. A case would need a rule
  for whose a line is when it belongs to two languages, which is the one genuinely new decision.
- **An opening tag split over two lines**, `<script\n  lang="ts">`, which is legal and rare. It
  counts as the shell today, deliberately, and a case would pin that as a choice rather than an
  accident. Cheap to write, worth having before anyone changes the opener detection.

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
- **The escaped apostrophe in unquoted shell text**, case 59. Shell's single quote is declared as a
  form that escapes nothing, which is true of its body and is what makes `sep='\'` come out right.
  The scan applies that answer to the opener as well, so a `\'` written outside quotes opens a
  string instead of being one apostrophe. Fixing it means asking whether a raw opener may still be
  cancelled by an escape while its closer may not, which is a change to the scan and touches Go,
  Odin and D as well, so it wants its own decision rather than a quiet edit to Shell.txt.

## Definitions rather than mistakes

Cases 23, 24 and 47 hold the three places where mezura's numbers differ from a region-based counter
by design: a blank line inside a block comment, a line holding only punctuation, and a blank line
inside a multi line string. They pass as they are and are here as a reminder that when another
tool's pair of numbers is added, those three are where the pairs will differ without either tool
being wrong. The third is the largest of them by far on real trees: it is the whole of the Python,
Shell and Perl disagreement over the Linux kernel.
