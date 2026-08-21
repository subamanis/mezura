# mezura

## About
This is a fairly <b>fast</b>, fairly <b>accurate</b>, very <b>customizable</b> stats generator for programming projects, in the form of a CLI executable, written in <b>Rust</b>.
It is used for counting total lines, code lines, user defined <b>keywords</b> like classes, enums, etc., visualize the statistics, and to track the growth of codebases.<br><br>
It is maintained on <b>Windows</b>, but it is expected to work on <b>Linux</b> and <b>MacOS</b>.

Example run on entire Linux Kernel: </br>
<img src="screenshots/example-linux.png" width="1000">



## Table of contents
* [How To Run](#how-to-run)
* [Details](#details)
* [Cmd Commands](#cmd-commands)
* [Scripting](#scripting)
* [Configuration Files](#configuration-files)
* [Logs and History](#logs-and-history)
* [Themes](#themes)
* [Supported Languages](#supported-languages)
* [Accuracy and Limitations](#accuracy-and-limitations)
* [Windows Performance Note](#windows-performance-note)
* [Similar Projects](#similar-projects)


## How To Run
The only thing you need is the binary, and there are 3 ways to get it:

### 1. Install it with cargo
```bash
cargo install --locked --git https://github.com/subamanis/mezura
```
To update an existing installation to the latest version, just run the same command again: it will detect that the repository has new commits, rebuild, and replace the old binary.

### 2. Build it yourself
After cloning or downloading the repo:
```bash
cargo build --release
```

### 3. Download the prebuilt binary
Grab the one for your platform from the [latest release](https://github.com/subamanis/mezura/releases/latest).
Windows, Linux and macOS binaries are built and tested on every tagged version.

<br>

And to run it:
```bash
mezura <optional_path> --optional_command1 --optional_commandN
```

The program expects none or many paths to some directories or code files, separated by comma, if more than one.
If no path is provided, the current working directory will be assumed as target directory.

The program also accepts a lot of optional flags to customize functionality, see [Cmd Commands](#cmd-commands) for more info or use the --help command.


## Details
The generated stats are the following:
- Number of files
- Lines, split into code, comments and everything else, with percentages
- Size (total and average) 
- Keyword occurrences
- Percentage comparisons between languages
- The same figures grouped by a named part of the project, if you name one (see the modules section of ```--targets```)
- Difference of stats between executions 

By default, the files and folders that are ignored by a .gitignore are skipped, so that build artifacts and dependencies don't pollute the stats (see the ```--no-gitignore``` command).

There is a "data" folder in the repository, that contains some already provided language files, themes and the default configuration file.
The program, at compile time, includes the "data" folder in the binary, and during the first execution, it saves it with the same structure in a persistent path, inside the user's computer, according to the platform's specification. More specifically, the paths per operating system are:
```
    Windows:  %APPDATA%\mezura
    Linux:    /home/$USER/.local/share/mezura
    MacOs:    /Users/$USER/Library/Application Support/mezura
```

The languages, themes, configurations and logs are then read from those folders, on that first execution as much as on every one after it, so the user can have easy access and modify them,
like add more languages of his choice, add custom themes, or modify the default configuration.

Installing a new version updates the language files there, so a correction to a language reaches you without you having to do anything. One that you changed yourself is replaced too, since a language file that has fallen behind counts wrongly, but your copy is kept under ```data/replaced/<version>/<date and time>/``` and the program names it, so you can carry your changes over. Each update or ```--restore``` writes its own folder there, so two of them never mix and the newest is the one at the bottom. A language file of your own is never touched, and neither are your themes, your default configuration or ```extension_priority.txt```: those are written when they are absent and left alone afterwards.

In order for a file to be considered for counting, some language file in the "data/languages" dir must claim it, either by its extension or by its whole name, in the 'Extensions' or 'Filenames' field, see [Supported Languages](#supported-languages). A file with no extension and a name nothing claims gets one more chance: its first line is read, and if it is a ```#!``` line naming an interpreter some language claims in its 'Shebangs' field, the file is counted as that language, which is how a ```deploy``` or ```configure``` script is counted instead of silently skipped. The interpreter is found past ```/usr/bin/env``` and its flags, and a versioned name falls back to its plain one, so ```#!/usr/bin/env python3.12``` counts as Python.


## Cmd Commands
Below there is a list with all the commands-flags that the program accepts.
```
WHAT IS COUNTED

--targets
    the directories and files to count, and the names to group them under (modules)

    1..n paths to directories or files, separated by commas:
    '--targets <path1>, <path2>'

    A path can be a glob pattern (* ? [..] {..}), so 'services/*/src' is a target. A path that
    exists exactly as written is taken literally, so a folder named with one of those characters
    is still a folder. A target inside another target is dropped, so nothing is counted twice.

    A path you write out is always counted, even if it is ignored, dotted or a link. The matches
    of a pattern were found by mezura rather than named by you, so those are skipped like any
    other found path (see '--no-gitignore' and '--search-in-dotted').

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
    'the tests there, the rest of the project here' in either order. What no name claimed becomes
    a row called '(unnamed)' and comes last; the rest keep the order you wrote them in, which is
    also the order of the columns in 'matrix'. '--sort' orders the languages inside a module and
    never the modules.

    Once anything is named a space ends a target, so a path holding spaces cannot be written
    beside modules: the shell takes the quotation marks away before mezura sees them. Put those
    in a configuration file, one target per line.

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

--exclude
    paths to leave out, as glob patterns

    1..n glob patterns separated by commas.

    A pattern without a slash matches a file or folder name at any depth ('node_modules', '*.min.js').
    A pattern with slashes matches the end of the full path, anchored at path components
    ('Rusty/mezura' matches '.../Rusty/mezura' but not '.../aRusty/mezura'). Full absolute
    paths work too. Glob syntax is supported in both forms: * ? [..] {..}
    Matching folders are skipped entirely; the files inside them are not traversed and
    are not included in the reported count of excluded files.

    If you are using Windows Powershell, you will need to escape the commas with a backtick: `
    or surround all the arguments with quotation marks:
    <arg1>`, <arg2>`, <arg3>   or   "<arg1>, <arg2>, <arg3>"

--languages
    count only these languages and leave every other one out of the report

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' dir gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    An extension that two languages claim names whichever of them owns it for this run, which is
    the answer in 'extension_priority.txt' or the one '--force-language' gave.

--exclude-languages
    count everything except these languages

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' dir gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    A name that nothing in the scan answers to is reported as having changed nothing, and the run
    carries on.

--force-language
    count an extension as the language you pick, even if another one claims it

    1..n pairs of 'extension=language' or 'filename=language' separated by commas, case-insensitive

    '--force-language m=matlab,pl=perl,txt=python'

    A whole filename works the same way, for the files that have no extension worth reading:
    '--force-language Makefile=python,Jenkinsfile=groovy'

    Overrides the 'extension_priority.txt' file of the data dir.

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

--search-in-dotted
    go into directories whose name starts with a dot

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Directories like '.vscode' and '.github' are skipped without this flag.

    The '.git' directory is never traversed, with or without this command, at any depth. Nothing
    inside it is source, and walking it is thousands of files for no count at all.

--show-languages
    print the languages this installation knows and stop

    No arguments.

    Lists by name what is in the 'data/languages/' directory, and counts nothing. Adding a file
    there teaches mezura another language.

HOW THE REPORT LOOKS

--layout
    the shape of the details section: a table, a box, a list, or a matrix of modules

    One argument: 'table', 'boxed', 'list' or 'matrix'. Default: table

      table     one aligned row per language: Language, Files, Lines, Code, Comments, Extra
                and Size, with a percentage next to each of the first four. Aligned with
                spaces and no borders, so it pastes into a README or a ticket unchanged.
      boxed     the same figures inside a drawn frame. Each number shares a cell with its
                percentage, which makes it narrower than 'table'. Needs a terminal that can
                draw box characters.
      list      one block of three rows per language: the file count and the size above the
                name, the line breakdown beside it, the keywords below. Wider, and it cannot
                be read down a column, but it reads well for a handful of languages.
      matrix    languages down, modules across, one number per cell, so a row says how the
                modules compare on the same language. That one number is whatever '--sort'
                is ordering by, named in a line above the table, and a dash is a language
                the module does not have. With no module named there is nothing to cross,
                so it says so and prints 'table' instead.

    The percentage beside 'Files' and 'Lines' is the language's share of the whole scan. The one
    beside 'Code' and 'Comments' is its share of that language's own lines.

    In the two tables the keywords cannot be a column without destroying the alignment, so they
    are printed as their own block underneath, one line per language. '--hide keywords' still
    suppresses them.

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
    language declares. The JSON document carries the same rows under each language, as 'by_file'.

--hide
    parts of the output to leave unprinted

    One or more names separated by commas or spaces, for example:
    --hide parsing-info,timing   or   --hide parsing-info timing

    What you can hide:

      version         the version line at the top
      directory-info  the 'Analyzing directories' line and the 'N files found' line under it
      parsing-info    the 'Parsing files' line and the 'ok' under it
      progress-bar    the bar, the share done and the speed figures of a long parse, keeping
                      its file count
      animations      every moving line: the scan's dots, the live progress bar and the working
                      lines of a '--diff'. What they settle into still prints, and a TERM=dumb
                      terminal hides them on its own
      keywords        the keyword counts, keeping the rest of the details rows
      nested-languages  the rows that break a container file down, so a '.vue' weighs whole on
                      the Vue row with no sign of the TypeScript and CSS inside it
      overview        the whole percentages section
      bar             only the [-|||-] bar of the overview, keeping the percentages and the colors
      history         the comparison with previous runs (the same as '--compare 0')
      timing          the execution time line at the bottom
      files           the files column of the details rows
      comments        the comments column of the details rows
      extra           the third column of the details rows, which is what '--counting content'
                      leaves after code and comments
      blanks          the same column under '--counting region'. Each word belongs to one model,
                      so naming the other one's hides nothing and says so
      size            the size column of the details rows, and the size half of the 'list'
                      layout's files line
      percentages     every percentage of the details rows, keeping the numbers they describe

    The column names reach every layout except 'matrix', whose three rows stay whole. Hiding the
    column '--sort' orders by falls back to sorting by lines, and says so.

    A '--diff' comparison obeys them too, taking each change away with its figure. Its
    percentages are percentages of the change, so hiding those leaves the absolute move, and it
    has no 'extra' column to hide.

    Errors and warnings are never hidden, and a hidden parsing info still reports the files that
    failed to parse, since the numbers would otherwise be wrong with nothing saying so.

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

--style
    override the colour and attributes of one kind of printed text

    One or more 'token=style' pairs separated by commas, for example:
    --style code-number=bright-black,code-label=b5a98a italic,heading=white bold underline

    A style is a color and any number of the attributes 'bold', 'italic', 'underline', 'dim' and
    'reverse', in any order. The color is a hex value, one of the 16 terminal color names, or
    'default' to leave the terminal's own foreground alone. 'reverse' swaps the text and
    background colors, so it stands out without picking one.

    The cells of the live progress bar take two forms no other token does: hex values separated
    by '..' fill them with an even gradient, and 'rainbow' walks a spectrum along them. A gradient
    needs hex values, since a colour name has no shade to interpolate. Every other token takes one
    colour and says so if given either form.

    Every counted quantity has two tokens, one for the figure and one for the word beside it:

      files-number  files-label             comments-number  comments-label
      lines-number  lines-label             total-size-number  total-size-label
      code-number  code-label               avg-size-number  avg-size-label
      extra-number  extra-label             keyword-number  keyword-label

    The "history" section counts the same quantities and takes the same tokens. 'size-unit' is
    the 'KB' of '430.5 KB total', one token for both sizes, kept apart from the labels so it can
    stay quiet while 'Size' reads like any column header.

    The rest, by where they appear.

    The page:
      version                  the version line at the top
      heading                  the section titles and the 'Analyzing directories' lines
      separator-total          the line above the total
      separator-header         the line under the column titles of the two tables
      summary                  the found / of interest / excluded line
      note                     the '(+N more languages hidden by --top N)' line
      success                  the 'ok' after parsing
      warning                  warnings
      error                    errors
      footer                   the execution time line

    The details tables:
      details-language-header  the word 'Language' over the first column of the two tables
      details-language-name    the name of a language, in a row and in the keywords block
      details-module           the name of a module, wherever one is printed
      details-total            the word 'Total'
      percent                  the percentages of the details rows
      sort-marker              the arrow beside the title of the column '--sort' ordered by
      arrow                    the '->' after a language name, in the 'list' layout only

    The rows hanging under a language, one token per column, twice over: 'nested-' for the
    sections inside a container file, 'file-' for the rows of a '--by-file' run:

      nested-name  nested-branch  nested-files  nested-lines  nested-code  nested-comments
      nested-extra  nested-size  nested-size-unit  nested-percent
      file-name  file-branch  file-files  file-lines  file-code  file-comments
      file-extra  file-size  file-size-unit  file-percent

    'name' is the section's language or the file's path, 'branch' the tree characters tying the
    row to the one above, and 'percent' is of the container for a section and of its language
    for a file.

    An '--explain' run. Its verdict words follow the label tokens above, the first two below
    paint stretches of the source lines, and anything neither keeps the terminal's own color:
      explain-string           the stretches of a line that sit inside a string
      explain-comment          the stretches that sit inside a comment
      explain-detail           the class name on a verdict row

    The overview:
      overview-label           the 'Files:', 'Lines:' and 'Size :' row labels
      overview-percent         the percentages of the overview
      bar-frame                the brackets around the overview bar and the live one
      language-1 language-2 language-3 language-4
                               each language of the bar, its name and the colour of its cells.
                               The fourth shows only when nothing was folded into 'others'
      language-others          the folded 'others' entry, which falls back to 'language-4'
                               where a theme names that one and not this

    A figure that moved, in the history section and in a '--diff' comparison alike:
      change-up  change-down  change-same

    The history section, which compares this run with the ones before it:
      history-entry            the '->' of an entry
      history-modified         the word 'modified:' on an entry counted with other settings
      history-modified-field   the names of the settings that changed since that entry

    The live lines, which only a terminal ever sees:
      progress-bar-fill        the cells the progress bar has drawn
      progress-bar-empty       the cells it has not reached, a faint track behind the whole
                               bar. 'default' takes the track away and leaves them blank
      progress-bar-figures     the file counts, the share done and the speed figures

    Only the color of a 'language-' token reaches the cells of the overview bar; bold, italic and
    the rest apply to the language name alone.

    A theme file and the style block of a config take the same tokens, and each wins over the
    last: built-in defaults, then the theme, then the config, then '--style' for this run.

--bar-thickness
    the character the overview's percentage bar is drawn with

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: medium

      slim     |   plain ASCII, so it renders on any terminal
      medium   ┃   thicker, and still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a font that can draw box characters. If the bar comes out as question
    marks or empty boxes, use 'slim'.

--progress-bar
    the characters the live progress bar is drawn with

    One argument: 'smooth', 'blocky' or 'hash'. Default: smooth

      smooth   ▏▎▍▌▋▊▉█   one unbroken bar, its tip moving in eight steps per cell
      blocky   ▪▮         separate boxes, each narrower than its cell, so a small gap falls
                          between them
      hash     .:#        plain ASCII, so it renders on any terminal

    The bar only appears on a terminal, on a parse long enough to watch, with the share done
    beside it; '--hide progress-bar' keeps its file count and drops the rest.

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

--decimal-separator
    the character before the decimals of every printed number

    One argument: 'dot' or 'comma'. The character itself is also accepted, so
    '--decimal-separator ,' is the same as '--decimal-separator comma'. Default: dot

    It applies to the sizes, the percentages and the execution time.

    It may be the same character '--number-separator' groups the digits with, since both
    conventions are in use somewhere. What is written to a log file is not affected, so a log
    stays readable by any version.

--show-themes
    print the themes this installation holds, each previewed, and stop

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: medium

    Lists by name what is in the 'data/themes/' directory, and counts nothing. Each one is drawn
    on a sample of real details rows and a mock overview, in the shape '--layout' asks for, so a
    theme is judged the way it will be printed.

    The optional argument is a '--bar-thickness' for the preview bar.

--theme-editor
    open a page for tuning the language colours of every theme, and stop

    No arguments.

    Writes an HTML page carrying the language colours of every theme in your 'data/themes'
    directory, opens it in your browser, and counts nothing. Every colour can be moved there,
    against a live contrast reading and a mock overview drawn with the bar character the program
    prints, and the page hands back the five 'language-' lines to paste into a theme file.

TAKING THE RESULT ELSEWHERE

--output
    text for a person, or one JSON document for another program

    One argument: 'text' or 'json'. Default: text

    'json' replaces the whole output, status lines and overview included, with a single document,
    so another program can read the run.

      mezura ./src --output json > stats.json
      mezura ./src --output json | jq '.total.code'

    The counts are plain numbers of lines and bytes, with no thousands separators, no KB or MB,
    no percentages and no colours, whatever the other settings say. '--sort' and '--top' still
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

--log
    append this run to the log of the loaded configuration

    Can take 0..n words as arguments in the cmd.

    Only works with a configuration loaded, since the log belongs to it: the entry is appended to
    that configuration's file in the 'data/logs' directory, and the file is created if it is not
    there yet. Any words you give are kept with the entry as its description.

    Cannot be saved in a configuration file, so loading one never writes an entry on its own. A
    '--diff' run is not logged either, and says so instead of writing an entry.

COMPARING WITH EARLIER RUNS

--compare
    how many earlier logged runs to show the difference against

    1 argument: a number between 0 and 10. Default: 1

    Only works with a configuration loaded, since the entries being compared against are the ones
    '--log' wrote under it. 0 turns the comparison off.

    Every log entry records the settings that decide what is counted, and an entry that was written
    with different ones is marked 'modified:' followed by their names. The comparison is still shown,
    because the point is to say whether it can be trusted: a change of 'targets' means the numbers came
    from another tree, while a change of 'exclude' means part of that tree stopped being counted and
    the rest of it did not move. '--counting' is not among them, since an entry records where every
    line landed rather than one fold of it and is read under whichever model this run is showing. An
    entry written by a version that did not record a setting is never reported as having changed it.

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

    '--sort' and '--top' order and cut the rows as ever. Only 'table' and 'boxed' can draw a
    comparison, so the other two layouts fall back to the table.

    A document records the settings it was counted with, and this run takes them so both sides
    cover the same tree: a baseline taken with '--exclude target' excludes it here too, and says
    so. Type a setting yourself and yours wins, with a warning that the two were not taken alike.
    The same warning covers two documents that disagree, or two counted by different versions.
    '--counting' never warns, since both sides are folded from the same record. '--no-gitignore'
    cannot reach a revision, and mezura says so. A document written with '--top' is refused, since
    the languages it left out would read as deleted.

    With '--output json' the comparison is a document of its own, the same vocabulary as a run's,
    every count carrying 'from', 'to' and 'change' and the two readings named under 'from' and
    'to'. '--top' does not cut it.

    Cannot be saved in a configuration file.

YOUR DATA DIRECTORY

--save
    save the flags of this run as a named configuration

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    The run happens as normal, and a .txt file of that name is written into 'data/config/' holding
    the flags it ran with. '--load <name>' brings them back.

--load
    take the flags of this run from a saved configuration file

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Reads a file '--save' wrote in the 'data/config/' directory and applies its flags. Anything you
    also type on the command line wins over what the file says.

    Give '--load' and '--save' the same name to edit a configuration: it is loaded, your changes
    are applied on top, and the result is written back.

--save-theme
    save the way this run looks as a named theme

    One argument, the name of the theme file to write (case-insensitive, no extension).

    Writes everything about the way this run looks into a theme file: whatever theme was loaded,
    plus the style block of the configuration, plus any '--style' given on the command line, all
    flattened into values. The file stands on its own and can be shared as it is.

    Combined with '--save', the configuration that is written points at this theme by name and
    carries no styles of its own.

--show-configs
    print the configurations this installation holds and stop

    No arguments.

    Lists by name what is in the 'data/config/' directory, and counts nothing. Any of them is
    loaded with '--load <name>'.

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
    changes over, a fresh folder per run. A language file of your own is never touched, and
    neither are your themes or your default configuration: those are written when absent and left
    alone.

    'extension_priority.txt' is merged instead. Each line names an extension that several
    languages claim, and the first language on the line wins it. Your lines are kept as they are,
    and lines for extensions your copy never mentions are added. To change a winner, reorder the
    names on its line; deleting the line brings it back, since a missing line and a line you never
    had look the same. Your copy is saved under 'replaced' whenever anything is added to it.

TUNING AND DIAGNOSTICS

--explain
    show one file line by line instead of printing a report

    No arguments. The target must be exactly one file.

    Every line is shown with the bucket it lands in, the class mezura read off it, and, where
    something was still open when the line began, what that was and where it started: 'in a
    comment opened by /* on line 23', 'in a string opened by " on line 7'. A line read by an
    embedded language names it. The stretches inside a string or a comment are printed in their
    own styles, which '--style' reaches as 'explain-string' and 'explain-comment', so a symbol
    swallowed by a string can be seen to be one.

      mezura src/main.rs --explain
      mezura src/page.vue --explain --counting region
      mezura src/main.rs --explain --output json

    A total can be right by accident, two mistakes cancelling out, and a per-line answer cannot,
    which is what this is for: checking a surprising count against the file itself.

    '--output json' writes the same answer as a document with one verdict per line and nothing
    else on the output, and no log entry is written. Each verdict carries those stretches as
    'spans', byte offsets into its line. '--counting' picks the buckets, and the commands
    choosing what is counted ('--languages', '--force-language' and the rest) apply as in a run.
    The commands that belong to a report over a whole scan ('--diff', '--log', '--compare',
    '--sort', '--top', '--by-file') are refused beside it.

--threads
    how many threads walk the directories and how many parse the files

    2 numbers: the first between 1 and 32 and the second between 1 and 128.

    The producers walk the directories you named, the consumers parse the files they find. Without
    this command both numbers are chosen from the threads your machine has.

    The default asks for far more consumers than cores on purpose. A consumer spends most of its
    life waiting for a file to open, so the speed comes from how many reads are in flight, not
    from how many cores there are. Raising it costs nothing on a fast disk whose files are already
    cached, and is worth up to twice the speed on a slow one, or on the first run after a reboot.

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

THE PROGRAM ITSELF

--help
    this list, or the full help of the commands you name

    No arguments, 'full', or any number of command names written with their dashes.

    On its own it prints one line per command. Name commands to read those in full and nothing
    else, '--help --style --layout'. 'full' prints every command in full, which is long.

    Nothing is counted either way.

--version
    the version of this binary and the day it was released

    No arguments.

    Prints the version of this binary and the date it was released on, and counts nothing. An
    unreleased build says so instead of naming a date.

    Not to be confused with '--hide version', which only leaves the version line off the top of
    a normal run.

--changelog
    what changed in this version, or in every version with 'full'

    No arguments, or the optional argument 'full'.

    Prints what changed in the version you are running, and counts nothing. With 'full' it prints
    every version before it as well, the newest first.
```


## Scripting

`--output json` writes a single JSON document instead of the printed report, so that a build step, a
badge or a dashboard can read a run instead of a person. Everything that is not the document itself,
warnings included, goes to the error output, so `mezura ./src --output json > stats.json` leaves a file
that a parser accepts. The same holds for `--explain --output json`, where the document answers for
one file with one verdict per line. The document is written even when there was nothing to count, and even when
every file failed to parse: a consumer never has to tell "no output" apart from "no code found", and
a run that failed says so in the document instead of leaving an empty file.

```
mezura ./src --output json | jq '.total.code'
mezura ./src --output json | jq -r '.languages[] | "\(.name) \(.lines)"'
```

The counts are plain numbers of lines and bytes, with no separators, no KB or MB and no percentages:
those are decisions about a terminal and a consumer that wants them can compute them. `scope` echoes
the settings that can change a number, so that two documents are not compared when one of them was
produced with a different `--exclude` or `--counting`. Every total, language, section and file row
also carries a `classes` block holding the nine raw per-line counts both counting models fold from,
so a consumer can compute either model whatever the scope names. `format` is the version of the
document itself, separate from `mezura_version`, and it only moves when a key is removed or changes
meaning, so a parser can check that one and ignore which build wrote the file.

`faulty_files` names every file that was found and could not be parsed, and `unreadable_dirs` every
directory the scan could not open, whose whole contents are therefore missing from every number in
the document. Both are arrays of objects carrying the path and the reason, so a permission is told
apart from a directory that was deleted while the scan was running. Both are empty on an ordinary
run, and either being non-empty means the counts are short by something the document names. Both are always written in full, whether or
not `--show-faulty-files` was given: that flag is a decision about how much to print on a terminal,
and two runs over the same code have to produce the same document.

`warnings` carries what the run said on the error output, which whoever reads the document never
sees. Each entry has a `code` that is safe to branch on, a `message` that is safe to show and free to
be reworded, the `subject` it is about, and an `affects` of `counts` or `settings`. That last one is
the useful question: an unreadable language file means a whole language went uncounted, while a
setting that was ignored leaves every number intact, and a consumer can gate on it without keeping a
list of every code that exists.

```
mezura ./src --output json | jq -e '[.warnings[] | select(.affects == "counts")] | length == 0'
```

**Exit codes.** 0 means mezura ran and told you what it found, including when it found nothing,
because zero is an answer. 1 means the run failed: a mistake in what was asked for, a name that does
not exist, a set of files where every one of them failed to be parsed, or a scan that found nothing
after failing to open a directory, since that zero is not a count of anything. The failing cases
still write the whole JSON document, faulty files and unreadable directories included, so the
failure can be read and not only detected.


## Configuration Files
If we plan to run the program many times for a project, it can be bothersome to specify all the flags every time, especially if they contain a lot of target and exclude dirs.
That's why you can specify many flags in a <b>*configuration file*</b>, and have the program just load that file (see the --load command). <br>

Configurations can be created automatically by specifying all the flags once, along with the command "--save", and a name for the configuration. Then the program, along with its normal execution, will automatically create a config file with the name you specified, and dump all the flags in there. <br>
The next time you want to run the program on this project, you can do it like this: 
```mezura --load <config_name>``` <br>

By default, there is a configuration file name "default" already present in the "data/config" dir, that gets loaded on every run. There, you can customize your preferences and they will apply to all runs, except if overriden by explicitely providing a different flag in the cmd, or by loading a specific configuration. For example, if you prefer the counting model of the other counters, you can put a "===> counting" block holding "region" there. <br>

The priorities of the specified flags are:
1) cmd
2) Specific config file
3) Default config file
4) Internal defaults



## Logs and History
Inside the 'data/logs' folder, the program will save log files that correspond to saved configurations everytime the '--log' flag is used. <br>
A log is a .jsonl file: one JSON entry per line, the newest first, so it is read by any JSON tool one line at a time. Each entry records the date and time of the execution and the name of the log (if specified), the settings the run was counted with (the target directories, the counting model, and so on, so you can see if at some point the configuration got modified), the total files, lines and size, and the raw per-line counts that the code and comment columns are folded from, so the history section shows every entry under whatever "--counting" the current run uses. <br>

A run that names its targets records its modules in the entry, and the history section then carries one narrow line per module: which of them grew, and by how much. A module that was not there last time, or that is not there any more, is named as such instead of being compared against nothing. An entry written by a run that named none has no such block. <br>

By using the '--compare <N>' flag, the (N) previous logged executions will be retrieved from the file and will be compared and printed to the screen. For example
for N = 3, it would look like this:
![](screenshots/compare-logs.PNG)

Note that a configuration file must be loaded for both '--log' and '--compare' to work.



## Themes
Everything the program prints can be styled: 46 tokens, each taking a color plus any of bold, italic, underline, dim and reverse. A color is either a hex value or one of the 16 standard terminal color names, which follow the color scheme of your terminal. Run ```--help style``` for the full list of tokens.

A **theme** is a plain .txt file of ```token = value``` lines, in the "data/themes" dir of the persistent data path. It carries only how the output looks, never what is measured, so it can be shared as it is. Apply one with ```--theme <name>```, list the ones you have with ```--show-themes```, and write the current look into a new one with ```--save-theme <name>```.

7 themes are bundled: **Mezura** (the default one), Dracula, Gruvbox, Catppuccin, Meadow, Neon and Ocean. Edit them, or add your own by dropping a file there.

The four languages of the overview and the folded 'others' entry are five ordinary tokens, ```language-1``` to ```language-4``` and ```language-others```, so a theme sets them the same way it sets everything else.

To make picking those five easier, there is an interactive editor, where every theme is previewed on a mock overview, every color can be adjusted (with live contrast and color distance metrics, so that the result stays readable), and the outcome is turned into the lines you paste into a theme file.

<b>[Open the theme editor online](https://subamanis.github.io/mezura/theme-editor/)</b> to play with the bundled themes, or run ```mezura --theme-editor``` to open it with the themes found on your own machine, including the ones you created.

<a href="https://subamanis.github.io/mezura/theme-editor/"><img src="screenshots/palette-tuner.png" width="900"></a>

## Supported Languages
Note that the default supported languages are incomplete, but they can be easily expanded by the user. All the supported languages can be found in the folder "data/languages"
as separate text files, in the persistent data path of the application. 
The user can easily specify a new language by copying the file of a language that looks like theirs and editing it.

Header files have their own dedicated languages: `.h` files are counted under "C Header" and `.hpp` files under "C++ Header", since the program cannot know which codebase a header belongs to.

If two or more language files claim the same extension, the winner is the one named in the `extension_priority.txt` file of the data dir, which ships with an answer for every contest between the languages that come with the program. An extension that nobody has named there goes to the language that comes first alphabetically, and the program reports it, since that is a tie-break and not a decision. Either way ```--force-language``` overrides it for a single run or, through a configuration file, for a single project.

**[The language files guide](LANGUAGE_FILES_GUIDE.md)** is a page of its own: a whole language file to copy, every block with an example, which of the five string blocks a symbol belongs in, and the two mistakes that cost people the most time.

One thing worth knowing before you open it: the blocks are read in one fixed order, and one that arrives out of place, or one header misspelt, makes the whole file unreadable and leaves that language out of the run. That is deliberate. A typo taken as "ignore this block" would change your counts without a word.

	
## Accuracy and Limitations

Before the details, the one decision that explains most of the difference between mezura's numbers and another counter's: **by default, mezura asks what a line says, not where it sits.** A blank line inside a block comment is blank, not comment, because it tells you nothing about the documentation around it. A line holding only ```}``` or ```);``` is neither code nor comment, because the language required it and the programmer said nothing by writing it. Counters that group by region answer the other question, "which block is this line inside", and give the blank line to the comment and the brace to the code. Neither reading is wrong, they answer different questions, and it is worth knowing which one you are reading: under the default, ```code``` and ```comments``` do not add up to ```lines```, and what is left over is the part of the file that carries nothing. Both answers come from the same run, and ```--counting region``` shows the other one: any code on the line makes it code, a line inside a comment belongs to the comment whatever it holds, the extra column gives way to blanks, and the columns then line up with what cloc, tokei and scc print. Measured over the 125 C files of the Linux kernel's ```mm/``` directory, ```--counting region``` and tokei agree on every column of every file.

The program is able to understand and parse correctly arbitrarily complex code structures with intertwined strings and comments. This way it can identify if a line contains something other than a comment, even if the comment is partitioned in multiple positions and it can identify valid keywords, that are not inside strings or comments.
For example in a line like ```/*class"*/" class" aclass```, it will not count "class" as a keyword since the first is inside a comment, the second inside a string and the third has a prefix.
Additionally:
- It checks for escaped characters, for example ```/"``` will not be counted as a string symbol
- It resolves symbols that are side by side, for example ```*/*``` is normally identified as both a closing and an opening comment symbol, but the program will understand the correct usage.
- A language that writes comments in more than one way gets all of them, and each one ends only with its own closing symbol. Pascal writes ```{ }``` and ```(* *)```, D writes ```/* */``` and ```/+ +/```, and a stray ```*)``` inside a ```{ }``` comment is just text.
- A comment written inside another comment does not end it early. This is how OCaml, Rust, Haskell, F#, Scala, Kotlin, Swift, Julia, Elm, Lisp, Scheme, MATLAB and WebAssembly text read their own code, so commenting out a block that already had comments in it counts as the comment it is. In C and the languages that follow it the first ```*/``` really does end the comment, and that is what the program does there.
- Lua's ```--[==[``` comments are read to their real end, so a ```]]``` written inside one is text.
- A comment that ends and another that begins on the same line (```]]--[[``` in Lua, ```--><!--``` in HTML) are read as two comments.
- Strings are read with the same care. A quote left open by mistake costs its own line instead of everything below it, a Python docstring or a JavaScript backtick still runs for as many lines as it likes, and the raw forms that end with a different symbol than they started with, like Rust's ```r#"..."#``` or C#'s ```@"..."```, end where the language says they do even when they finish with a backslash.

With that said, it is important to mention the following limitations:

- Sections of another language inside a file are counted with their own symbols where the language file declares them: the ```<script>``` and ```<style>``` blocks of HTML, Vue and Svelte, with ```lang="ts"``` and its kin naming the section's language and the tag's default answering when nothing does. A Vue template deliberately stays Vue, since its directives are not HTML. What remains outside: languages that interleave mid-line, the ```<?php ?>``` of PHP and the ERB/JSP/Blade family, which are still read as one language from beginning to end, and an opener tag split over two lines, which stays with its file's language.

- Keywords are counted as words, not as meaning. Wherever ```class``` appears as a word in code it is counted, and in a language that uses the same word for a second purpose those occurrences are counted too. Mezura has no idea what a declaration is; it knows where the code is and which words you asked it to look for.

- If a target path contains another target path, the contained one is dropped, so that its files are not counted twice. A symbolic link (or a Windows junction) that the scan comes across is not followed, for the same reason: whatever it points at would be counted a second time through it. The same goes for one that a glob pattern matched, since those are found by the program rather than named by you. One that you name as a target yourself is followed, since that is what you asked for. Hard links are the case that stays: they are indistinguishable from an ordinary file, so the same content reached through two of them is counted twice.

- A string delimiter that the programmer invents on the spot cannot be written in a language file, since it is different every time it is used: a shell or PHP heredoc, Rust's ```r##"..."##``` past the one-hash form, and Lua's levelled ```[=[ ]=]``` string brackets past the plain ```[[ ]]``` form, which is declared. What is inside them is plain text to the language, while mezura is still counting quotes in it, so one apostrophe in an ordinary sentence written inside a heredoc is enough to look like the start of a string. In most languages the damage ends with that line. In the ones where a string may legally run over several lines (Rust, Ruby, the shells, PHP, SQL and their kin) mezura cannot tell a long string from a lone quote, and everything below reads as string content until the next quote turns up.

- Everything in a language file stands for itself, with one exception: the two characters ```=*``` inside a comment symbol mean "any number of ```=``` here". It is what lets a single line declare Lua's ```--[[```, ```--[=[``` and ```--[==[``` together, and it is a small cheat: a language whose comment symbol really contained ```=*``` could not be written down. No such language is known, and if one turns up the marker moves to a block of its own.

- Extensions are matched without regard to case, so a file named ```MAIN.RS``` is counted as Rust, and so is one named ```main.rs```. The one thing this loses is the Unix convention where an upper case ```.C``` means C++ while ```.c``` means C: to mezura they are the same extension.

- When two languages claim the same extension or the same filename, only one of them can have it, and the choice changes the numbers rather than only the label, since the loser's files are then parsed with the winner's comment and string symbols. Mezura makes no attempt to settle it by looking inside the files: a guess from the contents would be right most of the time, and the times it was wrong it would be wrong quietly, in the middle of a run, differently for two files with the same extension. So it takes the answer from you and says so every time it has no answer to take. To decide it once, name the winner in the ```extension_priority.txt``` file of the data directory, under ```contested-extensions``` or ```contested-filenames```; to decide it for one run or for one project, use ```--force-language```, which overrides that file and takes a filename as readily as an extension.

- A regular expression written inside a string is read as a string, escapes included, and creates no inaccuracy. A regex literal, in the languages that have them, can miscount in one way: a comment opener sitting inside it, like the ```/*``` of JavaScript in ```/a[/*]b/```, reads as a comment that has opened, so the lines after it are counted as comment until something closes it. A bare quote inside a regex costs nothing, since those languages declare their quotes as ending with the line.


## Windows Performance Note

Opening a file is far more expensive on Windows than on Linux: every open walks the object manager, the security descriptor and the whole filter driver stack, which is where antivirus and other minifilters sit. Since mezura opens one file after another, this dominates: **on Windows the program is I/O bound, and most of its time is spent waiting on `File::open` rather than counting anything**. On Linux the same open is cheap, so the program is parsing bound and the time goes where it should, into the parser.

The practical consequence is that the same repository on the same machine is measurably faster to analyze from Linux (~2x speedup), and that on Windows the biggest wins come from removing work from the open path rather than from making the parser faster. Worth knowing when reading a profile of your own: a thread that is waiting for a core, and not for the disk, is still counted as waiting inside the open it was about to make, so a large `File::open` share is a question rather than a verdict.

That baseline cost is structural and does not go away. What can be removed is what sits on top of it: because every open traverses the filter stack, real-time antivirus protection ends up inside mezura's hot path, inspecting each file as it is opened, and it multiplies an already expensive operation. Excluding mezura from that scanning does not make Windows open files as cheaply as Linux does, it only removes the worst amplifier, which on a large tree is the difference between not-as-fast-as-could-be and slow.

### Removing the antivirus overhead
To exclude mezura from Windows Defender real-time scanning:

1. Open PowerShell as Administrator
2. Run:
   ```powershell
   Add-MpPreference -ExclusionProcess "mezura.exe"
   ```
3. Restart your terminal

To remove the exclusion later:
```powershell
Remove-MpPreference -ExclusionProcess "mezura.exe"
```

**Note:** This is a common requirement for file-scanning tools (similar to build tools, IDEs, etc.). Only add this exclusion if you trust the source of the executable.

**The exclusion is matched on the exact filename**, so a copy you renamed to anything else does not have it, and on this machine that alone measured 1.28x on a large repository with byte-identical binaries. Worth knowing if you keep several builds around, or if you ever time one against another.


## Similar Projects

If you don't require the keyword counting functionality of this program, the history tracking feature, or the alternate-than-usual visualization, use the [scc](https://github.com/boyter/scc) project written in GO, that is honestly impressive.

Other alternative projects you can check are:
- [loc](https://github.com/cgag/loc)
- [cloc](https://github.com/AlDanial/cloc)
- [sloc](https://github.com/flosse/sloc)
- [tokei](https://github.com/XAMPPRocky/tokei)

<br>

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
