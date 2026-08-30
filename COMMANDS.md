# Commands

The full help of every command, exactly as `mezura --help <command>` prints it. A test writes this file from the help texts themselves, so do not edit it by hand. Regenerate it with `MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura commands_document`.

- [What is counted](#what-is-counted)
  - [--targets](#cmd-targets)
  - [--counting](#cmd-counting)
  - [--exclude](#cmd-exclude)
  - [--languages](#cmd-languages)
  - [--exclude-languages](#cmd-exclude-languages)
  - [--force-language](#cmd-force-language)
  - [--no-gitignore](#cmd-no-gitignore)
  - [--no-ignore-files](#cmd-no-ignore-files)
  - [--search-in-dotted](#cmd-search-in-dotted)
  - [--count-minified](#cmd-count-minified)
  - [--count-generated](#cmd-count-generated)
  - [--show-languages](#cmd-show-languages)
- [How the report looks](#how-the-report-looks)
  - [--layout](#cmd-layout)
  - [--sort](#cmd-sort)
  - [--top](#cmd-top)
  - [--by-file](#cmd-by-file)
  - [--hide](#cmd-hide)
  - [--theme](#cmd-theme)
  - [--style](#cmd-style)
  - [--bar-thickness](#cmd-bar-thickness)
  - [--progress-bar](#cmd-progress-bar)
  - [--number-separator](#cmd-number-separator)
  - [--decimal-separator](#cmd-decimal-separator)
  - [--show-themes](#cmd-show-themes)
  - [--theme-editor](#cmd-theme-editor)
- [Taking the result elsewhere](#taking-the-result-elsewhere)
  - [--output](#cmd-output)
  - [--log](#cmd-log)
- [Comparing with earlier runs](#comparing-with-earlier-runs)
  - [--compare](#cmd-compare)
  - [--diff](#cmd-diff)
- [Your data directory](#your-data-directory)
  - [--save](#cmd-save)
  - [--load](#cmd-load)
  - [--save-theme](#cmd-save-theme)
  - [--show-configs](#cmd-show-configs)
  - [--restore](#cmd-restore)
- [The settings of a project](#the-settings-of-a-project)
  - [--save-local](#cmd-save-local)
  - [--no-local](#cmd-no-local)
- [Tuning and diagnostics](#tuning-and-diagnostics)
  - [--explain](#cmd-explain)
  - [--threads](#cmd-threads)
  - [--show-faulty-files](#cmd-show-faulty-files)
- [The program itself](#the-program-itself)
  - [--help](#cmd-help)
  - [--version](#cmd-version)
  - [--changelog](#cmd-changelog)

## What is counted

### <a id="cmd-targets" name="cmd-targets"></a>--targets

```
--targets
    the directories and files to count, and the names to group them under (modules)

    1..n paths to directories or files, separated by commas:
    '--targets <path1>, <path2>'

    A path can be a glob pattern (* ? [..] {..}), so 'services/*/src' is a target. A path that
    exists exactly as written is taken literally, so a directory named with one of those characters
    is still a directory. A target inside another target is dropped, so nothing is counted twice.

    A path you write out is always counted, even if it is ignored, dotted, a link, minified or
    generated. The matches of a pattern were found by mezura rather than named by you, so those
    are skipped like any other found path (see '--no-gitignore', '--search-in-dotted',
    '--count-minified' and '--count-generated').

    In Windows Powershell the commas need a backtick, or the whole list needs quotation marks:
    <path1>`, <path2>`, <path3>   or   "<path1>, <path2>, <path3>"

    Targets can also be given as the first arguments of the program, or in a configuration file.

    MODULES

    Give a target a name and the report is grouped by it as well as by language:

        mezura frontend=./web backend=./api
        mezura ./project tests=./project/tests

    A comma continues one module and a space ends it, so 'tests=./api/tests,./web/tests' is one
    module of two directories while 'frontend=./web ./ui' is the module and a separate unnamed
    target. Repeating a name adds to it, and one path under two names is refused.

    Every file belongs to one module and the most specific path wins, so the second example means
    'the tests there, the rest of the project here' in either order. Files that were not claimed by
    a module explicitly become a row called '(unnamed)' and it comes last; the rest keep the order you wrote them in, which is
    also the order of the columns in 'matrix'. '--sort' orders the languages inside a module and
    never the modules.

    Once anything is named a space ends a target, so a path holding spaces cannot be written
    beside modules: the shell takes the quotation marks away before mezura sees them. Put those
    in a configuration file, one target per line.
```

### <a id="cmd-counting" name="cmd-counting"></a>--counting

```
--counting
    whether a line counts by where its words are or by where the line sits

    One argument, 'content' or 'region'. Default: content

    Which counting model the code, comments and third column follow. Both models are answered by
    the same run: this only chooses which one the columns show.

    'content' counts a line where its words are. Words in code make it code, words only inside a
    comment make it a comment, and a line with no word anywhere is extra, whether it is blank, a
    lone brace like '}' or '});', or a bare '*/'. Writing 'if (x) { do(); }' on one line or on
    three does not change the number, which keeps the count honest as a measure of content.

    'region' counts a line where it sits, the way cloc, tokei and scc count. Any code on the line
    makes it code, a line inside a comment belongs to the comment whatever it holds, a blank
    inside a comment or a string counts with its region, and the extra column gives way to
    blanks: only an empty line outside everything is blank. Use this model when comparing
    mezura's numbers against another counter's.
```

### <a id="cmd-exclude" name="cmd-exclude"></a>--exclude

```
--exclude
    paths to leave out, as glob patterns

    1..n glob patterns separated by commas.

    A pattern without a slash matches a file or directory name at any depth ('node_modules', '*.min.js').
    A pattern with slashes matches the end of the full path, anchored at path components
    ('Rusty/mezura' matches '.../Rusty/mezura' but not '.../aRusty/mezura'). Full absolute
    paths work too. Glob syntax is supported in both forms: * ? [..] {..}
    Matching folders are skipped entirely; the files inside them are not traversed and
    are not included in the reported count of excluded files.

    If you are using Windows Powershell, you will need to escape the commas with a backtick: `
    or surround all the arguments with quotation marks:
    <arg1>`, <arg2>`, <arg3>   or   "<arg1>, <arg2>, <arg3>"
```

### <a id="cmd-languages" name="cmd-languages"></a>--languages

```
--languages
    count only these languages and leave every other one out of the report

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' directory gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    An extension that two languages claim names whichever of them owns it for this run, which is
    the answer in 'language_conflicts.txt' or the one '--force-language' gave.

    Writing a module and a slash before the name holds it to that module alone, and a module that
    names any language of its own counts those and nothing else: '--languages rust,web/js' counts
    Rust everywhere but inside 'web', where it counts JavaScript.
```

### <a id="cmd-exclude-languages" name="cmd-exclude-languages"></a>--exclude-languages

```
--exclude-languages
    count everything except these languages

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' directory gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    A name that nothing in the scan answers to is reported as having changed nothing, and the run
    carries on.

    Writing a module and a slash before the name holds it to that module alone, and a module that
    names any language of its own leaves out those and nothing else: '--exclude-languages json,web/xml'
    leaves out JSON everywhere but inside 'web', where it leaves out XML and counts the JSON.
```

### <a id="cmd-force-language" name="cmd-force-language"></a>--force-language

```
--force-language
    count an extension as the language you pick, even if another one claims it

    1..n pairs of 'extension=language' or 'filename=language' separated by commas, case-insensitive

    '--force-language m=matlab,pl=perl,txt=python'

    A whole filename works the same way, for the files that have no extension worth reading:
    '--force-language Makefile=python,Jenkinsfile=groovy'

    Writing a module and a slash before the extension holds the rule to that module alone, so one
    repository with MATLAB in one folder and Objective-C in another is counted once:
    'mezura ios=./ios analysis=./matlab --force-language ios/m=objective-c,analysis/m=matlab'
    A module keeps every rule you wrote without a module in front of it and answers only the ones
    it names itself. The module is spelled exactly as the target that declares it.

    Overrides the 'language_conflicts.txt' file of the data directory.
```

### <a id="cmd-no-gitignore" name="cmd-no-gitignore"></a>--no-gitignore

```
--no-gitignore
    count the files a .gitignore ignores

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Without this flag a .gitignore is obeyed: anything it ignores is skipped and counted among the
    excluded files. The .gitignore files read are the ones inside the directories being walked and
    the ones above them, up to the repository root, and negated patterns ('!keep.log') work.

    A path you wrote out yourself is always counted, even where a .gitignore above it says
    otherwise. The files a glob pattern matched do not count as written out, since mezura is the
    one that found them.

    A .ignore or .rgignore is obeyed too, and is turned off separately with '--no-ignore-files'.
```

### <a id="cmd-no-ignore-files" name="cmd-no-ignore-files"></a>--no-ignore-files

```
--no-ignore-files
    count the files a .ignore or a .rgignore hides

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    These are the two ignore files that ripgrep, the silver searcher and fd read and that git does
    not. They are written in the same format as a .gitignore and are read from the same places, and
    they exist for the opposite need: hiding something from your tools while keeping it in version
    control, which is what a vendored dependency or a committed bundle is.

    Kept apart from '--no-gitignore' because they are two decisions. Obeying the repository and
    obeying whoever set up the search tools are not the same thing, and one flag for both would
    leave nobody able to say 'obey git, ignore the search tools'.

    Where they disagree, the last word goes to the file with the narrowest audience: a rule in
    .rgignore beats one in .ignore, which beats one in .gitignore, in the directory they share.
```

### <a id="cmd-search-in-dotted" name="cmd-search-in-dotted"></a>--search-in-dotted

```
--search-in-dotted
    go into directories whose name starts with a dot

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Directories like '.vscode' and '.github' are skipped without this flag.

    The '.git' directory is never traversed, with or without this command, at any depth. Nothing
    inside it is source, and walking it is thousands of files for no count at all.
```

### <a id="cmd-count-minified" name="cmd-count-minified"></a>--count-minified

```
--count-minified
    count the minified files that are left out by default

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    A bundle is a whole program written on one or two lines, so counting it puts a file of forty
    thousand real lines into the report as one line of code. A file is taken as minified when its
    lines average 1000 bytes or more; files under 10 KB are never tested, since they cannot move a
    report either way. The figure is set well above the widest real source there is, generated
    bindings padded into columns, which reach 350.

    A file left out is reported above the table and appears in no figure. With this flag every
    file is counted as it is written.
```

### <a id="cmd-count-generated" name="cmd-count-generated"></a>--count-generated

```
--count-generated
    count the generated files that are left out by default

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    A file written by a tool says so in its head, and that is the whole of the test: 'do not edit',
    'auto-generated', 'autogenerated' or '@generated' anywhere in the first 512 bytes. It catches
    protobuf output, register maps, ORM models and bindings, which nobody wrote and nobody reads.

    The 512 bytes are not a saving, they are the accuracy: read deeper and what turns up is the
    generators themselves, whose own source holds the marker they print.

    A file left out is reported above the table and appears in no figure.
```

### <a id="cmd-show-languages" name="cmd-show-languages"></a>--show-languages

```
--show-languages
    print the languages this installation knows and stop

    No arguments.

    Lists by name what is in the 'data/languages/' directory, and counts nothing. Adding a file
    there teaches mezura another language.

    A name on the list that cannot count anything is reported under it: two files declaring one
    language, and a language whose every extension was given to another one by
    'language_conflicts.txt', which is the file to reorder to hand one back.
```

## How the report looks

### <a id="cmd-layout" name="cmd-layout"></a>--layout

```
--layout
    the shape of the details section: a table, a box, a list, or a matrix of modules

    One argument: 'table', 'boxed', 'list' or 'matrix'. Default: table

      table     one aligned row per language: Language, Files, Lines, Code, Comments, Extra
                and Size, with a percentage next to each of the first four. Aligned with
                spaces and no borders, so it pastes into a README or a ticket unchanged.
      boxed     the same figures inside a drawn frame. Each number shares a cell with its
                percentage, which makes it narrower than 'table'. Needs a terminal that can
                draw box characters.
      list      one row per language, reading left to right: the file count, the line count,
                then how the lines split, with the size after a '|' at the end. The keywords
                hang under it. It cannot be read down a column, and it leaves out the third
                quantity that the two tables carry.
      matrix    languages down, modules across, one number per cell, so a row says how the
                modules compare on the same language. That one number is whatever '--sort'
                is ordering by, named in a line above the table, and a dash is a language
                the module does not have. With no module named there is nothing to cross,
                so it says so and prints 'table' instead.

    The percentage beside 'Files' and 'Lines' is the language's share of the whole scan. The one
    beside 'Code' and 'Comments' is its share of that language's own lines. A percentage that comes
    out zero is left out, since the count next to it is already a zero.

    In the two tables the keywords cannot be a column without destroying the alignment, so they
    are printed as their own block underneath, one line per language. '--hide keywords' still
    suppresses them.
```

### <a id="cmd-sort" name="cmd-sort"></a>--sort

```
--sort
    which column the languages are ordered by

    One argument: 'lines', 'files', 'code', 'comments', 'extra', 'blanks', 'size' or 'name'.
    Default: lines
    Every column of the details table is one of them, so there is no figure you can see and not
    order by. The third column is 'extra' under '--counting content' and 'blanks' under
    '--counting region', and naming the other model's word orders by lines and says so.

    Orders the languages in the "details" section, which also decides which of them reach the
    "overview" section and which are folded into its 'others' entry.

    Everything except 'name' sorts from the largest down, and ties are broken alphabetically so
    the order never changes between runs on the same data. The column that decides it carries a
    mark in its header, since the criterion can come from a configuration file and then nothing
    else on the page would say it.
```

### <a id="cmd-top" name="cmd-top"></a>--top

```
--top
    show only this many languages, and say how many were left out

    One number, 1 or greater.

    Shows only that many languages in the "details" section, the ones that come first under
    '--sort'. A line underneath says how many were hidden, so the rows never fail to add up to the
    total without saying why. The total itself still counts every language. The "overview" section
    shows no more languages than this either, so asking for the top 2 does not leave a third one
    sitting in the bar.

    The modules keep the order you wrote them in, and the cut is made inside each one, since the
    rows under a module are its own languages. The 'matrix' layout is the exception: its rows are
    the languages of the whole run, so there the cut is over all of them.
```

### <a id="cmd-by-file" name="cmd-by-file"></a>--by-file

```
--by-file
    give every file its own row, or only the biggest few of each language

    No arguments, or one number.

    Every file gets a row under the language it was counted as, with its lines split into code,
    comments and everything else. Without a number, every file is printed.

    A number is how many to show under each language: '--by-file 20' prints the twenty biggest of
    every language, ordered by whatever '--sort' is in effect, and 0 means all of them again. The
    cut is made inside each language of each module, for the same reason '--top' cuts inside each
    module: across a whole report, the one part with the biggest files would leave the others
    with none.

    A language whose files were cut ends on a branch left hanging, so a tree drawn shut means
    nothing was left out, and the count of what was is printed above the total. A language that
    '--top' hid has no row for its files to sit under, so they are not printed either.

    A path too wide for its column loses whole directories out of its middle and never a piece of
    the file's own name. A file's keywords are not counted, so a row is one line whatever the
    language declares. The JSON document carries the same rows under each language, as 'by_file',
    and a run with modules carries them inside each module's own languages.

    Under '--diff' the rows are the files that changed: each carries its move beside its figures,
    and a file only one side has is marked 'new' or 'gone'. The number then keeps the biggest
    moves of each language instead of the biggest files, measured by whatever '--sort' names, and
    by lines where it names the file count, since every row here is one file. The two sides pair
    their files by name, relative to each side's own targets, so readings whose targets differ in
    shape cannot pair.
    A JSON baseline must have been written with a plain '--by-file': one without rows, or one
    whose rows a number capped, is missing files that would all read as new, and mezura says so
    instead of comparing them.
```

### <a id="cmd-hide" name="cmd-hide"></a>--hide

```
--hide
    parts of the output to leave unprinted

    One or more names separated by commas or spaces, for example:
    --hide parsing-info,timing   or   --hide parsing-info timing

    What you can hide:

      version         the version line at the top
      directory-info  the 'Analyzing targets' line and the 'N files found' line under it
      parsing-info    the 'Parsing files' line and the 'ok' under it
      progress-bar    the bar, the share done and the speed figures of a long parse, keeping
                      its file count
      animations      every moving line: the scan's dots, the live progress bar and the working
                      lines of a '--diff'. What they settle into still prints, and a TERM=dumb
                      terminal hides them on its own
      keywords        the keyword counts, keeping the rest of the details rows. This one also
                      stops them being counted, so it is the only name here that makes a run faster
      nested-languages  the rows that break a container file down, so a '.vue' weighs whole on
                      the Vue row with no sign of the TypeScript and CSS inside it
      overview        the whole percentages section
      bar             only the [|||] bar of the overview, keeping the percentages and the colors
      history         the comparison with previous runs (the same as '--compare 0')
      timing          the execution time line at the bottom
      files           the files column of the details rows
      comments        the comments column of the details rows
      extra           the third column of the details rows, which is what '--counting content'
                      leaves after code and comments. The 'list' layout does not print it at
                      all, so there it hides nothing
      blanks          the same column under '--counting region'. Each word belongs to one model,
                      so naming the other one's hides nothing and says so
      size            the size column of the details rows, and the size that closes a 'list' row
      percentages     every percentage of the details rows, keeping the numbers they describe

    The column names reach every layout except 'matrix', whose three rows stay whole. Hiding the
    column '--sort' orders by falls back to sorting by lines, and says so.

    A '--diff' comparison obeys them too, taking each change away with its figure. Its
    percentages are percentages of the change, so hiding those leaves the absolute move, and it
    has no 'extra' column to hide.

    Errors and warnings are never hidden, and a hidden parsing info still reports the files that
    failed to parse, since the numbers would otherwise be wrong with nothing saying so.
```

### <a id="cmd-theme" name="cmd-theme"></a>--theme

```
--theme
    apply a theme, which is a whole look kept in one file

    One argument, the name of a theme (case-insensitive).

    A theme is a .txt file in the 'data/themes' directory, named by its file name. Every line is
    a 'token = value' pair, the same tokens and values '--style' takes (see '--help --style').
    Add your own there; '--show-themes' lists the ones you have, each one drawn.

    A theme carries only how the output looks. What is measured and what is shown stays in a
    configuration file, so a theme can be handed to someone else without carrying your paths
    or your settings with it.

    A style that does not parse is reported and skipped, and the rest of the theme still applies.
    A name that matches no file is an error, since that one is a mistake in the command.
```

### <a id="cmd-style" name="cmd-style"></a>--style

```
--style
    override the color and attributes of one kind of printed text

    One or more 'token=style' pairs separated by commas, for example:
    --style code-number=bright-black,code-label=b5a98a italic,heading=white bold underline

    A style is one or two colors and any of 'bold', 'italic', 'underline', 'dim' and 'reverse'.
    A color is a hex value, a terminal color name, or 'default' to leave it to the terminal. The
    first color is the text, the second the background behind it.

    Run '--theme-editor' to see most of these as an interactive webpage.

    The tokens, in the order their text appears on the screen.

    The top of the run:
      version                  the version line at the top
      note                     the asides: the settings of a project, what '--top' hid, the
                               notes above the total
      heading                  the section titles and the 'Analyzing targets' lines
      summary                  the found / of interest / excluded line
      success                  the 'ok' after parsing
      warning                  warnings, wherever one is printed
      error                    errors, wherever one is printed

    The live lines. Their cells alone also take two hex values as 'a..b' for an even gradient,
    or 'rainbow':
      progress-bar-fill        the cells the progress bar has drawn
      progress-bar-empty       the cells it has not reached, a faint track behind the whole
                               bar. 'default' takes the track away and leaves them blank
      progress-bar-figures     the file counts, the share done and the speed figures

    The header row of the details:
      details-language-header  the word 'Language' over the first column of the two tables
      files-label  lines-label  code-label  comments-label  extra-label  total-size-label
                               the titles of the counted columns
      sort-marker              the arrow beside the title of the column '--sort' ordered by
      separator-header         the line under the column titles of the two tables

    The rows of the details:
      details-language-name    the name of a language, in a row and in the keywords block
      details-module           the name of a module, wherever one is printed
      files-number  lines-number  code-number  comments-number  extra-number  total-size-number
                               the figures of the counted columns, here and in the history section
      size-unit                the 'KB' of '430.5 KB'
      percent                  the percentages of the details rows
      arrow                    the '->' and the '|' of a 'list' row, in that layout only

    The rows hanging under a language, one token per column, twice over: 'nested-' for the
    sections inside a container file, 'file-' for the rows of a '--by-file' run. 'name' is the
    section's language or the file's path, 'branch' the tree characters tying the row to the one
    above, and 'percent' is of the container for a section and of its language for a file:

      nested-name  nested-branch  nested-files  nested-lines  nested-code  nested-comments
      nested-extra  nested-size  nested-size-unit  nested-percent
      file-name  file-branch  file-files  file-lines  file-code  file-comments
      file-extra  file-size  file-size-unit  file-percent

    The total:
      separator-total          the line above the total
      details-total            the word 'Total'

    The keywords:
      keyword-label            the name of a keyword
      keyword-number           its count

    The overview:
      overview-label           the 'Files:', 'Lines:' and 'Size:' row labels
      overview-percent         the percentages of the overview
      bar-frame                the brackets around the overview bar and the live one
      language-1  language-2  language-3  language-4
                               each language of the bar, its name and the color of its cells.
                               The fourth shows only when nothing was folded into 'others'. Only
                               the color reaches the cells; bold and the rest apply to the name
      language-others          the folded 'others' entry, which falls back to 'language-4'
                               where a theme names that one and not this

    The history section, whose figures take the same number tokens as the table:
      history-entry            the '->' of an entry
      history-age              the '(2 days, 3 hours and 5 minutes ago)' of an entry
      history-label            the 'Files:', 'Lines:', 'Code:' and 'Comments:' words of an entry
      history-modified         the word 'modified:' on an entry counted with other settings
      history-modified-field   the names of the settings that changed since that entry
      change-up  change-down  change-same
                               a figure that moved, here and in a '--diff' comparison alike

    The last line:
      footer                   the execution time line

    An '--explain' run. The two span tokens paint stretches of the source lines, and anything
    none of these names keeps the terminal's own color:
      explain-heading          the file line at the top
      explain-string           the stretches of a line that sit inside a string
      explain-comment          the stretches that sit inside a comment
      explain-code             the word 'code' on a verdict row
      explain-comments         the word 'comments' on a verdict row
      explain-extra            the third quantity's word on a verdict row, 'extra' or 'blanks'
      explain-detail           the class name on a verdict row

    A theme file and the style block of a config take the same tokens, and each wins over the
    last: built-in defaults, then the theme, then the config, then '--style' for this run.
```

### <a id="cmd-bar-thickness" name="cmd-bar-thickness"></a>--bar-thickness

```
--bar-thickness
    the character the overview's percentage bar is drawn with

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: medium

      slim     |   plain ASCII, so it renders on any terminal
      medium   ┃   thicker, and still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a font that can draw box characters. If the bar comes out as question
    marks or empty boxes, use 'slim'.
```

### <a id="cmd-progress-bar" name="cmd-progress-bar"></a>--progress-bar

```
--progress-bar
    the characters the live progress bar is drawn with

    One argument: 'smooth', 'blocky' or 'hash'. Default: smooth

      smooth   ▏▎▍▌▋▊▉█   one unbroken bar, its tip moving in eight steps per cell
      blocky   ▪▮         separate boxes, each narrower than its cell, so a small gap falls
                          between them
      hash     .:#        plain ASCII, so it renders on any terminal

    The bar only appears on a terminal, on a parse long enough to watch, with the share done
    beside it; '--hide progress-bar' keeps its file count and drops the rest.
```

### <a id="cmd-number-separator" name="cmd-number-separator"></a>--number-separator

```
--number-separator
    the character between the thousands of every printed number

    One argument: 'comma', 'underscore', 'dot' or 'none'. The character itself is also
    accepted, so '--number-separator _' is the same as '--number-separator underscore'.
    Default: comma

      comma        1,559,486
      underscore   1_559_486
      dot          1.559.486
      none         1559486

    The keyword row lists several figures next to each other, separated by commas, so
    'comma' is the one choice where a grouped number and the end of one are the same
    character.
```

### <a id="cmd-decimal-separator" name="cmd-decimal-separator"></a>--decimal-separator

```
--decimal-separator
    the character before the decimals of every printed number

    One argument: 'dot' or 'comma'. The character itself is also accepted, so
    '--decimal-separator ,' is the same as '--decimal-separator comma'. Default: dot

    It applies to the sizes, the percentages and the execution time.

    It may be the same character '--number-separator' groups the digits with, since both
    conventions are in use somewhere. What is written to a log file is not affected, so a log
    stays readable by any version.
```

### <a id="cmd-show-themes" name="cmd-show-themes"></a>--show-themes

```
--show-themes
    print the themes this installation holds, each previewed, and stop

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: medium

    Lists by name what is in the 'data/themes/' directory, and counts nothing. Each one is drawn
    on a sample of real details rows and a mock overview, in the shape '--layout' asks for, so a
    theme is judged the way it will be printed.

    The optional argument is a '--bar-thickness' for the preview bar.
```

### <a id="cmd-theme-editor" name="cmd-theme-editor"></a>--theme-editor

```
--theme-editor
    open a page for tuning the colors of the report, and stop

    No arguments.

    Writes an HTML page, opens it in your browser, and counts nothing. It shows one run of mezura
    in the colors of every theme in your 'data/themes' directory, and hands back the lines to paste
    into a theme file or into the style block of a configuration.
```

## Taking the result elsewhere

### <a id="cmd-output" name="cmd-output"></a>--output

```
--output
    text for a person, or one JSON document for another program

    One argument: 'text' or 'json'. Default: text

    'json' replaces the whole output, status lines and overview included, with a single document,
    so another program can read the run.

      mezura ./src --output json > stats.json
      mezura ./src --output json | jq '.total.code'

    The document is written on one line, without spacing to look at. To read one yourself, pipe
    it through 'jq .'

    The counts are plain numbers of lines and bytes, with no thousands separators, no KB or MB,
    no percentages and no colors, whatever the other settings say. '--sort' and '--top' still
    order and cut the languages, and the count of the ones left out is in the document. Of the
    '--hide' list only 'keywords' and 'timing' apply, since the rest name printed sections a JSON
    run does not have.

    Warnings and errors go to the error output, so no stray line lands inside the document, and
    the warnings are in it as well under 'warnings', each with a 'code' safe to branch on, an
    'affects' of 'counts' or 'settings', a 'subject' and a readable 'message'. 'affects' is what
    says whether the numbers can be trusted: an unreadable language file means a whole language
    went uncounted, an ignored setting touches nothing. The list is there even when empty, and a
    run that found nothing still writes a valid document with a total of zero.

    Cannot be saved in a configuration file.
```

### <a id="cmd-log" name="cmd-log"></a>--log

```
--log
    append this run to the log of the loaded configuration

    Can take 0..n words as arguments in the cmd.

    The log belongs to a set of settings rather than to a run: with a configuration loaded the entry
    is appended to that configuration's file in the 'data/logs' directory, and inside a project with
    a '.mezura' folder it goes to the log in that folder, beside the code. With neither, there is
    nothing for the entry to belong to and the command says so instead of writing one. The file is
    created if it is not there yet, and any words you give are kept with the entry as its
    description.

    Cannot be saved in a configuration file, so loading one never writes an entry on its own. A
    '--diff' run is not logged either, and says so instead of writing an entry.
```

## Comparing with earlier runs

### <a id="cmd-compare" name="cmd-compare"></a>--compare

```
--compare
    how many earlier logged runs to show the difference against

    1 argument: a number between 0 and 10. Default: 1

    Reads the entries '--log' wrote, so it needs the same thing that command needs: a configuration
    loaded, or a project with a '.mezura' folder to be inside. 0 turns the comparison off.

    Every log entry records the settings that decide what is counted, and an entry that was written
    with different ones is marked 'modified:' followed by their names. The comparison is still shown,
    because the point is to say whether it can be trusted: a change of 'targets' means the numbers came
    from another tree, while a change of 'exclude' means part of that tree stopped being counted and
    the rest of it did not move. '--counting' is not among them, since an entry records where every
    line landed rather than one fold of it and is read under whichever model this run is showing. An
    entry written by a version that did not record a setting is never reported as having changed it.
```

### <a id="cmd-diff" name="cmd-diff"></a>--diff

```
--diff
    what changed since an earlier run, or between two of them

    One argument: a reading, or two of them with '..' between, oldest first. A reading is the
    path of a JSON document that an earlier run wrote, or a git revision: a branch, a tag, or
    enough of a commit hash to be unique.

    The comparison takes the place of the report rather than sitting under it.

      mezura ./src --output json > baseline.json
      mezura ./src --diff baseline.json
      mezura ./src --diff main
      mezura ./src --diff v2.0.1..v3.0.0
      mezura --diff january.json..june.json

    A name that is a file on disk is read as a document, anything else is asked of git, which
    resolves 'origin/main' as readily as a tag. Only one '..' is allowed and it is always the
    separator, so '--diff ../old.json' works while a path climbing twice is refused.

    A revision is counted on the spot with this run's settings and targets, so 'mezura ./src
    --diff main' counts what './src' held on 'main'. It must already be fetched, and every target
    must be in one git repository. A directory the revision does not have counts as zero, so all
    of it reads as new.

    Counting a revision is slower, since the commit is written out to a temporary directory
    first. Size can move there while no line did, when git gives the checkout different line
    endings from your working tree.

    Every figure shows what it is now, how much it moved, and by what percentage. A figure that
    did not move is a dash. There is no 'Extra' column. Keywords are marked the same way and only
    where one moved: 'structs: 57 (+5), traits: 2'. The overview and the history section are not
    printed.

    A language only one reading has is marked 'new' or 'gone'. Modules get a row each when both
    readings named the same ones; when they did not, everything is compared at once and mezura
    says what each side named.

    '--sort' and '--top' order and cut the rows as ever. '--by-file' adds the files that changed
    under each language, marked 'new' and 'gone' the same way; its number keeps the biggest moves.
    Only 'table' and 'boxed' can draw a comparison, so the other two layouts fall back to the
    table.

    A document records the settings it was counted with, and this run takes them so both sides
    cover the same tree: a baseline taken with '--exclude target' excludes it here too, and says
    so. Type a setting yourself and yours wins, with a warning that the two were not taken alike.
    The same warning covers two documents that disagree, or two counted by different versions.
    '--counting' never warns, since both sides are folded from the same record. '--no-gitignore'
    cannot reach a revision, and mezura says so. A document written with '--top' is refused, since
    the languages it left out would read as deleted.

    With '--output json' the comparison is a document of its own, the same vocabulary as a run's,
    every count carrying 'from', 'to' and 'change' and the two readings named under 'from' and
    'to'. '--top' does not cut it, and '--by-file' adds the changed files to it, under 'by_file'.

    Cannot be saved in a configuration file.
```

## Your data directory

### <a id="cmd-save" name="cmd-save"></a>--save

```
--save
    save the flags of this run as a named configuration

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    The run happens as normal, and a .txt file of that name is written into 'data/config/' holding
    the flags it ran with. '--load <name>' brings them back.
```

### <a id="cmd-load" name="cmd-load"></a>--load

```
--load
    take the flags of this run from a saved configuration file

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Reads a file '--save' wrote in the 'data/config/' directory and applies its flags. Anything you
    also type on the command line wins over what the file says.

    Give '--load' and '--save' the same name to edit a configuration: it is loaded, your changes
    are applied on top, and the result is written back.

    Naming a configuration is asking for that one and no other, so a project's own settings are left
    out of a run that names one, and its log stays where every named configuration's log is.
```

### <a id="cmd-save-theme" name="cmd-save-theme"></a>--save-theme

```
--save-theme
    save the way this run looks as a named theme

    One argument, the name of the theme file to write (case-insensitive, no extension).

    Writes everything about the way this run looks into a theme file: whatever theme was loaded,
    plus the style block of the configuration, plus any '--style' given on the command line, all
    flattened into values. The file stands on its own and can be shared as it is.

    Combined with '--save', the configuration that is written points at this theme by name and
    carries no styles of its own.
```

### <a id="cmd-show-configs" name="cmd-show-configs"></a>--show-configs

```
--show-configs
    print the configurations this installation holds and stop

    No arguments.

    Lists by name what is in the 'data/config/' directory, and counts nothing. Any of them is
    loaded with '--load <name>'.
```

### <a id="cmd-restore" name="cmd-restore"></a>--restore

```
--restore
    put the data directory back to what this version ships, and stop

    No arguments.

    Anything missing is written, and a language file that no longer says what ours says is
    replaced. It reports what it did, and nothing is counted.

    This happens on its own whenever the mezura you run carries different files from the ones your
    data directory was given, so you should not need it. It is here for when something was damaged
    or deleted while the program itself stayed the same, which nothing else would notice.

    A language file you changed is replaced too, since one that has fallen behind counts wrongly,
    but your copy is saved under 'data/replaced/<version>/<date and time>/' so you can carry your
    changes over, a fresh directory per run. A language file of your own is never touched, and
    neither are your themes or your default configuration: those are written when absent and left
    alone.

    'language_conflicts.txt' is merged instead. Each line names an extension that several
    languages claim, and the first language on the line wins it. Your lines are kept as they are,
    and lines for extensions your copy never mentions are added. To change a winner, reorder the
    names on its line; deleting the line brings it back, since a missing line and a line you never
    had look the same. Your copy is saved under 'replaced' whenever anything is added to it.
```

## The settings of a project

### <a id="cmd-save-local" name="cmd-save-local"></a>--save-local

```
--save-local
    save the flags of this run as the settings of this project

    No arguments.

    Writes a '.mezura' folder beside the code, holding a 'config.txt' with the flags this run used.
    Every later run inside that directory or under it counts with those flags without being asked,
    and says which file it took them from. The paths it writes are relative to the project, so the
    folder means the same places after the code is cloned somewhere else.

    Written where the next run will look for it: into the folder this run found, from wherever
    inside the project the command was typed, and otherwise into a new one at the directory holding
    the targets. What the file already held and you did not type again is kept, so a second
    '--save-local' adds to the project's settings rather than replacing them.

    The targets are part of what a run used, so they are saved with the rest: typing one from inside
    a subdirectory writes that subdirectory as the project's target, and every later run inside the
    project then counts it and nothing else.
```

### <a id="cmd-no-local" name="cmd-no-local"></a>--no-local

```
--no-local
    ignore the settings of the project being counted

    No arguments.

    Counts as though the project had no '.mezura' folder: your own flags, your own default
    configuration, and no entry written to the project's log.
```

## Tuning and diagnostics

### <a id="cmd-explain" name="cmd-explain"></a>--explain

```
--explain
    show one file line by line instead of printing a report

    No arguments, one line number, or two with '..' between them, the way '--diff' separates two
    revisions. Either end can be left off. The target must be exactly one file.

      mezura src/main.rs --explain 1210..1230
      mezura src/main.rs --explain 1213
      mezura src/main.rs --explain 1210..

    Given lines, only those are printed and the rest of the file is still read, which is what makes
    the answer right: a comment that opened above them decides every line in them, and each such
    line says so. A last line past the end of the file is not a mistake. Two totals are printed
    instead of one, for the lines shown and for the whole file, since the file's own is the number
    you came to check.

    Every line is shown with the bucket it lands in, 'code', 'comments' or 'extra', and the class
    that put it there, 'words_in_code' or 'punctuation_in_comment', which is the raw count both
    counting models fold into those three. Where something was still open when the line began, it
    says what and where it started: 'in a comment opened by /* on line 23', 'in a string opened by
    " on line 7'. A line read by an embedded language names it. The stretches inside a string or a
    comment are printed in their own styles, which '--style' reaches as 'explain-string' and
    'explain-comment', so a symbol swallowed by a string can be seen to be one.

      mezura src/main.rs --explain
      mezura src/page.vue --explain --counting region
      mezura src/main.rs --explain --output json

    A total can be right by accident, two mistakes cancelling out, and a per-line answer cannot,
    which is what this is for: checking a surprising count against the file itself.

    '--output json' writes the same answer as a document with one verdict per line and nothing
    else on the output, and no log entry is written. Each verdict carries those stretches as
    'spans', byte offsets into its line. It always answers for the whole file, because a program
    reading it is written against one entry per line, so a run asking for lines and for the
    document at once is refused. '--counting' picks the buckets, and the commands choosing what is
    counted ('--languages', '--force-language' and the rest) apply as in a run. The commands that
    belong to a report over a whole scan ('--diff', '--log', '--compare', '--sort', '--top',
    '--by-file') are refused beside it.
```

### <a id="cmd-threads" name="cmd-threads"></a>--threads

```
--threads
    how many threads walk the directories and how many parse the files

    2 numbers: the first between 1 and 32 and the second between 1 and 128.

    The producers walk the directories you named, the consumers parse the files they find. Without
    this command both numbers are chosen from the threads your machine has.

    The default asks for far more consumers than cores on purpose. A consumer spends most of its
    life waiting for a file to open, so the speed comes from how many reads are in flight, not
    from how many cores there are. Raising it costs nothing on a fast disk whose files are already
    cached, and is worth up to twice the speed on a slow one, or on the first run after a reboot.
```

### <a id="cmd-show-faulty-files" name="cmd-show-faulty-files"></a>--show-faulty-files

```
--show-faulty-files
    name the files that could not be parsed, and what went wrong with each

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    A file can fail while it is being opened or while it is being read, and by far the most common
    reason is that it holds bytes that are not UTF-8. A directory can fail to open at all, usually
    over its permissions or because something removed it mid-scan. Either way the run continues and
    the report prints how many there were, since everything under a directory that could not be
    read is missing from every number above it.

    This flag adds the path of each one and the error it gave.

    '--output json' obeys it too: the two lists of paths are written only when it is given, while
    how many there were is in the 'scan' block either way, so a document without the lists never
    claims that nothing went wrong. A comparison document carries the counts of each side and no
    lists at all.
```

## The program itself

### <a id="cmd-help" name="cmd-help"></a>--help

```
--help
    this list, or the full help of the commands you name

    No arguments, 'full', or any number of command names written with their dashes.

    On its own it prints one line per command. Name commands to read those in full and nothing
    else, '--help --style --layout'. 'full' prints every command in full, which is long.

    Nothing is counted either way.
```

### <a id="cmd-version" name="cmd-version"></a>--version

```
--version
    the version of this binary and the day it was released

    No arguments.

    Prints the version of this binary and the date it was released on, and counts nothing. An
    unreleased build says so instead of naming a date.

    Not to be confused with '--hide version', which only leaves the version line off the top of
    a normal run.
```

### <a id="cmd-changelog" name="cmd-changelog"></a>--changelog

```
--changelog
    what changed in this version, or in every version with 'full'

    No arguments, or the optional argument 'full'.

    Prints what changed in the version you are running, and counts nothing. With 'full' it prints
    every version before it as well, the newest first.
```
