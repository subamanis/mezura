use std::fs;

use colored::{ColoredString, Colorize};
use mezura_core::{CountingModel, Language, RunError};
use mezura_core::language_file::{FaultyLanguageFile, LanguageDirParseError};

use super::config_manager::*;
use crate::paths::PERSISTENT_APP_PATHS;

static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");
const MESSAGE_WIDTH : usize = 110;
// The help paints itself with these rather than through the theme, because it is printed before a
// theme is chosen: 'main' answers the message-only commands above the line that sets one, so
// anything that does ask for a theme there is handed the default.
const HELP_COMMAND : (u8, u8, u8) = (110, 160, 220);
const HELP_TEXT : (u8, u8, u8) = (140, 140, 140);
// Far enough from the blue of a command, which stands beside it in the same lists, and washed out
// enough not to shout over the text it labels
const HELP_VALUE_NAME : (u8, u8, u8) = (132, 154, 138);
const LIST_INDENT : usize = 2;
// Where the help indents a list of values, one to a line, with its description beside it
const VALUE_LIST_INDENT : usize = 6;

// These constants need to be maintained along with the readme's commands
pub const TARGETS_HELP  :  &str =
"--targets
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
    <path1>`, <path2>`, <path3>   or   \"<path1>, <path2>, <path3>\"

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

";
pub const EXCLUDE_HELP  :  &str =
"--exclude
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
    <arg1>`, <arg2>`, <arg3>   or   \"<arg1>, <arg2>, <arg3>\"

";
pub const NO_GITIGNORE_HELP  :  &str =
"--no-gitignore
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

";
pub const NO_IGNORE_FILES_HELP  :  &str =
"--no-ignore-files
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

";
pub const LANGUAGES_HELP  :  &str =
"--languages
    count only these languages and leave every other one out of the report

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' directory gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    An extension that two languages claim names whichever of them owns it for this run, which is
    the answer in 'language_conflicts.txt' or the one '--force-language' gave.

    Writing a module and a slash before the name holds it to that module alone, and a module that
    names any language of its own counts those and nothing else: '--languages rust,web/js' counts
    Rust everywhere but inside 'web', where it counts JavaScript.

";
pub const EXCLUDE_LANGUAGES_HELP  :  &str =
"--exclude-languages
    count everything except these languages

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' directory gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    A name that nothing in the scan answers to is reported as having changed nothing, and the run
    carries on.

    Writing a module and a slash before the name holds it to that module alone, and a module that
    names any language of its own leaves out those and nothing else: '--exclude-languages json,web/xml'
    leaves out JSON everywhere but inside 'web', where it leaves out XML and counts the JSON.

";
pub const FORCE_LANGUAGE_HELP  :  &str =
"--force-language
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

";
pub const THREADS_HELP  :  &str =
"--threads
    how many threads walk the directories and how many parse the files

    2 numbers: the first between 1 and 32 and the second between 1 and 128.

    The producers walk the directories you named, the consumers parse the files they find. Without
    this command both numbers are chosen from the threads your machine has.

    The default asks for far more consumers than cores on purpose. A consumer spends most of its
    life waiting for a file to open, so the speed comes from how many reads are in flight, not
    from how many cores there are. Raising it costs nothing on a fast disk whose files are already
    cached, and is worth up to twice the speed on a slow one, or on the first run after a reboot.

";
pub const COUNTING_HELP  :  &str =
"--counting
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

";
pub const SEARCH_IN_DOTTED_HELP  :  &str =
"--search-in-dotted
    go into directories whose name starts with a dot

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Directories like '.vscode' and '.github' are skipped without this flag.

    The '.git' directory is never traversed, with or without this command, at any depth. Nothing
    inside it is source, and walking it is thousands of files for no count at all.

";
pub const COUNT_MINIFIED_HELP  :  &str =
"--count-minified
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

";
pub const COUNT_GENERATED_HELP  :  &str =
"--count-generated
    count the generated files that are left out by default

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    A file written by a tool says so in its head, and that is the whole of the test: 'do not edit',
    'auto-generated', 'autogenerated' or '@generated' anywhere in the first 512 bytes. It catches
    protobuf output, register maps, ORM models and bindings, which nobody wrote and nobody reads.

    The 512 bytes are not a saving, they are the accuracy: read deeper and what turns up is the
    generators themselves, whose own source holds the marker they print.

    A file left out is reported above the table and appears in no figure.

";
pub const EXPLAIN_HELP  :  &str =
"--explain
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
    \" on line 7'. A line read by an embedded language names it. The stretches inside a string or a
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

";

pub const SHOW_FAULTY_FILES_HELP  :  &str =
"--show-faulty-files
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

";
pub const HIDE_HELP  :  &str =
"--hide
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

";
pub const THEME_HELP  :  &str =
"--theme
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

";
pub const SAVE_THEME_HELP  :  &str =
"--save-theme
    save the way this run looks as a named theme

    One argument, the name of the theme file to write (case-insensitive, no extension).

    Writes everything about the way this run looks into a theme file: whatever theme was loaded,
    plus the style block of the configuration, plus any '--style' given on the command line, all
    flattened into values. The file stands on its own and can be shared as it is.

    Combined with '--save', the configuration that is written points at this theme by name and
    carries no styles of its own.

";
pub const SORT_HELP  :  &str =
"--sort
    which column the languages are ordered by

    One argument: 'lines', 'files', 'code', 'comments', 'extra', 'blanks', 'size' or 'name'.
    Default: lines
    Every column of the details table is one of them, so there is no figure you can see and not
    order by. The third column is 'extra' under '--counting content' and 'blanks' under
    '--counting region', and naming the other model's word orders by lines and says so.

    Orders the languages in the \"details\" section, which also decides which of them reach the
    \"overview\" section and which are folded into its 'others' entry.

    Everything except 'name' sorts from the largest down, and ties are broken alphabetically so
    the order never changes between runs on the same data. The column that decides it carries a
    mark in its header, since the criterion can come from a configuration file and then nothing
    else on the page would say it.

";
pub const TOP_HELP  :  &str =
"--top
    show only this many languages, and say how many were left out

    One number, 1 or greater.

    Shows only that many languages in the \"details\" section, the ones that come first under
    '--sort'. A line underneath says how many were hidden, so the rows never fail to add up to the
    total without saying why. The total itself still counts every language. The \"overview\" section
    shows no more languages than this either, so asking for the top 2 does not leave a third one
    sitting in the bar.

    The modules keep the order you wrote them in, and the cut is made inside each one, since the
    rows under a module are its own languages. The 'matrix' layout is the exception: its rows are
    the languages of the whole run, so there the cut is over all of them.

";
pub const BAR_THICKNESS_HELP  :  &str =
"--bar-thickness
    the character the overview's percentage bar is drawn with

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: medium

      slim     |   plain ASCII, so it renders on any terminal
      medium   ┃   thicker, and still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a font that can draw box characters. If the bar comes out as question
    marks or empty boxes, use 'slim'.

";
pub const PROGRESS_BAR_HELP  :  &str =
"--progress-bar
    the characters the live progress bar is drawn with

    One argument: 'smooth', 'blocky' or 'hash'. Default: smooth

      smooth   ▏▎▍▌▋▊▉█   one unbroken bar, its tip moving in eight steps per cell
      blocky   ▪▮         separate boxes, each narrower than its cell, so a small gap falls
                          between them
      hash     .:#        plain ASCII, so it renders on any terminal

    The bar only appears on a terminal, on a parse long enough to watch, with the share done
    beside it; '--hide progress-bar' keeps its file count and drops the rest.

";
pub const LAYOUT_HELP  :  &str =
"--layout
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

";
pub const RESTORE_HELP  :  &str =
"--restore
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

";
pub const OUTPUT_HELP  :  &str =
"--output
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

";
pub const DIFF_HELP  :  &str =
"--diff
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

";
pub const NUMBER_SEPARATOR_HELP  :  &str =
"--number-separator
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

";
pub const DECIMAL_SEPARATOR_HELP  :  &str =
"--decimal-separator
    the character before the decimals of every printed number

    One argument: 'dot' or 'comma'. The character itself is also accepted, so
    '--decimal-separator ,' is the same as '--decimal-separator comma'. Default: dot

    It applies to the sizes, the percentages and the execution time.

    It may be the same character '--number-separator' groups the digits with, since both
    conventions are in use somewhere. What is written to a log file is not affected, so a log
    stays readable by any version.

";
pub const STYLE_HELP  :  &str =
"--style
    override the color and attributes of one kind of printed text

    One or more 'token=style' pairs separated by commas, for example:
    --style code-number=bright-black,code-label=b5a98a italic,heading=white bold underline

    A style is one or two colors and any number of the attributes 'bold', 'italic', 'underline',
    'dim' and 'reverse', in any order. A color is a hex value, one of the 16 terminal color names,
    or 'default' to leave that half to the terminal.

    The colors are the one thing the order decides: the first is what the text is painted in and
    the second what it sits on, so 'details-total=white 223344' is white on a dark blue and
    'note=default 3a2f1e' puts a background behind text whose color the terminal chooses.
    'reverse' still swaps whatever the two end up being, which is how a token stands out without
    naming either.

    The cells of the live progress bar take two forms no other token does: hex values separated
    by '..' fill them with an even gradient, and 'rainbow' walks a spectrum along them. A gradient
    needs hex values, since a color name has no shade to interpolate. Every other token takes one
    color and says so if given either form. Both forms answer per cell of a run, so neither can be
    the color a span of text sits on: a background is always a single one.

    A background covers the characters it is given and nothing else, so in a table it stops at the
    text rather than filling the column: the numbers of a column line up on one side and the color
    behind them ends where each number does.

    Every counted quantity has two tokens, one for the figure and one for the word beside it:

      files-number  files-label             comments-number  comments-label
      lines-number  lines-label             total-size-number  total-size-label
      code-number  code-label               keyword-number  keyword-label
      extra-number  extra-label

      size-unit                the 'KB' of '430.5 KB'

    The \"history\" section counts the same quantities and takes the same tokens. The unit is kept
    apart from the labels so it can stay quiet while 'Size' reads like any column header.

    The rest, by where they appear.

    The page:
      version                  the version line at the top
      heading                  the section titles and the 'Analyzing targets' lines
      summary                  the found / of interest / excluded line
      note                     the asides about the count: what '--top' hid, what was left out of
                               it, and the settings of a project it was taken with
      success                  the 'ok' after parsing
      warning                  warnings
      error                    errors
      footer                   the execution time line

    The details tables:
      details-language-header  the word 'Language' over the first column of the two tables
      details-language-name    the name of a language, in a row and in the keywords block
      details-module           the name of a module, wherever one is printed
      details-total            the word 'Total'
      separator-header         the line under the column titles of the two tables
      separator-total          the line above the total
      percent                  the percentages of the details rows
      sort-marker              the arrow beside the title of the column '--sort' ordered by
      arrow                    the '->' and the '|' of a 'list' row, in that layout only

    The rows hanging under a language, one token per column, twice over: 'nested-' for the
    sections inside a container file, 'file-' for the rows of a '--by-file' run:

      nested-name  nested-branch  nested-files  nested-lines  nested-code  nested-comments
      nested-extra  nested-size  nested-size-unit  nested-percent
      file-name  file-branch  file-files  file-lines  file-code  file-comments
      file-extra  file-size  file-size-unit  file-percent

    'name' is the section's language or the file's path, 'branch' the tree characters tying the
    row to the one above, and 'percent' is of the container for a section and of its language
    for a file.

    An '--explain' run. The two span tokens paint stretches of the source lines, and anything
    none of these names keeps the terminal's own color:
      explain-heading          the file line at the top
      explain-string           the stretches of a line that sit inside a string
      explain-comment          the stretches that sit inside a comment
      explain-code             the word 'code' on a verdict row
      explain-comments         the word 'comments' on a verdict row
      explain-extra            the third quantity's word on a verdict row, 'extra' or 'blanks'
      explain-detail           the class name on a verdict row

    The overview:
      overview-label           the 'Files:', 'Lines:' and 'Size:' row labels
      overview-percent         the percentages of the overview
      bar-frame                the brackets around the overview bar and the live one
      language-1  language-2  language-3  language-4
                               each language of the bar, its name and the color of its cells.
                               The fourth shows only when nothing was folded into 'others'
      language-others          the folded 'others' entry, which falls back to 'language-4'
                               where a theme names that one and not this

    A figure that moved, in the history section and in a '--diff' comparison alike:
      change-up  change-down  change-same

    The history section, which compares this run with the ones before it:
      history-entry            the '->' of an entry
      history-age              the '(2 days, 3 hours and 5 minutes ago)' of an entry
      history-label            the 'Files:', 'Lines:', 'Code:' and 'Comments:' words of an entry
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

";
pub const THEME_EDITOR_HELP  :  &str =
"--theme-editor
    open a page for tuning the colors of the report, and stop

    No arguments.

    Writes an HTML page, opens it in your browser, and counts nothing. It shows one run of mezura
    in the colors of every theme in your 'data/themes' directory, and hands back the lines to paste
    into a theme file or into the style block of a configuration.

";
pub const SHOW_THEMES_HELP  :  &str =
"--show-themes
    print the themes this installation holds, each previewed, and stop

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: medium

    Lists by name what is in the 'data/themes/' directory, and counts nothing. Each one is drawn
    on a sample of real details rows and a mock overview, in the shape '--layout' asks for, so a
    theme is judged the way it will be printed.

    The optional argument is a '--bar-thickness' for the preview bar.

";
pub const LOG_HELP  :  &str =
"--log
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

";
pub const COMPARE_LEVEL_HELP  :  &str =
"--compare
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

";
pub const BY_FILE_HELP  :  &str =
"--by-file
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

";
pub const SAVE_HELP  :  &str =
"--save
    save the flags of this run as a named configuration

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    The run happens as normal, and a .txt file of that name is written into 'data/config/' holding
    the flags it ran with. '--load <name>' brings them back.

";
pub const LOAD_HELP  :  &str =
"--load
    take the flags of this run from a saved configuration file

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Reads a file '--save' wrote in the 'data/config/' directory and applies its flags. Anything you
    also type on the command line wins over what the file says.

    Give '--load' and '--save' the same name to edit a configuration: it is loaded, your changes
    are applied on top, and the result is written back.

    Naming a configuration is asking for that one and no other, so a project's own settings are left
    out of a run that names one, and its log stays where every named configuration's log is.

";
pub const SAVE_LOCAL_HELP  :  &str =
"--save-local
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

";
pub const NO_LOCAL_HELP  :  &str =
"--no-local
    ignore the settings of the project being counted

    No arguments.

    Counts as though the project had no '.mezura' folder: your own flags, your own default
    configuration, and no entry written to the project's log.

";
pub const CHANGELOG_HELP  :  &str =
"--changelog
    what changed in this version, or in every version with 'full'

    No arguments, or the optional argument 'full'.

    Prints what changed in the version you are running, and counts nothing. With 'full' it prints
    every version before it as well, the newest first.

";
pub const SHOW_LANGUAGES_HELP  :  &str =
"--show-languages
    print the languages this installation knows and stop

    No arguments.

    Lists by name what is in the 'data/languages/' directory, and counts nothing. Adding a file
    there teaches mezura another language.

";
pub const SHOW_CONFIGS_HELP  :  &str =
"--show-configs
    print the configurations this installation holds and stop

    No arguments.

    Lists by name what is in the 'data/config/' directory, and counts nothing. Any of them is
    loaded with '--load <name>'.

";

pub const VERSION_HELP  :  &str =
"--version
    the version of this binary and the day it was released

    No arguments.

    Prints the version of this binary and the date it was released on, and counts nothing. An
    unreleased build says so instead of naming a date.

    Not to be confused with '--hide version', which only leaves the version line off the top of
    a normal run.

";
pub const HELP_HELP  :  &str =
"--help
    this list, or the full help of the commands you name

    No arguments, 'full', or any number of command names written with their dashes.

    On its own it prints one line per command. Name commands to read those in full and nothing
    else, '--help --style --layout'. 'full' prints every command in full, which is long.

    Nothing is counted either way.

";

// The full help prints this, the lookup for one command searches it, and the close-match
// suggestions take their candidates from it, so a new command cannot reach one and miss the others.
pub const COMMAND_HELP : [(&str, &[(&str, &str)]); 8] = [
    ("What is counted", &[
        (TARGETS, TARGETS_HELP),
        (COUNTING, COUNTING_HELP),
        (EXCLUDE, EXCLUDE_HELP),
        (LANGUAGES, LANGUAGES_HELP),
        (EXCLUDE_LANGUAGES, EXCLUDE_LANGUAGES_HELP),
        (FORCE_LANGUAGE, FORCE_LANGUAGE_HELP),
        (NO_GITIGNORE, NO_GITIGNORE_HELP),
        (NO_IGNORE_FILES, NO_IGNORE_FILES_HELP),
        (SEARCH_IN_DOTTED, SEARCH_IN_DOTTED_HELP),
        (COUNT_MINIFIED, COUNT_MINIFIED_HELP),
        (COUNT_GENERATED, COUNT_GENERATED_HELP),
        (SHOW_LANGUAGES, SHOW_LANGUAGES_HELP),
    ]),
    ("How the report looks", &[
        (LAYOUT, LAYOUT_HELP),
        (SORT, SORT_HELP),
        (TOP, TOP_HELP),
        (BY_FILE, BY_FILE_HELP),
        (HIDE, HIDE_HELP),
        (THEME, THEME_HELP),
        (STYLE, STYLE_HELP),
        (BAR_THICKNESS, BAR_THICKNESS_HELP),
        (PROGRESS_BAR, PROGRESS_BAR_HELP),
        (NUMBER_SEPARATOR, NUMBER_SEPARATOR_HELP),
        (DECIMAL_SEPARATOR, DECIMAL_SEPARATOR_HELP),
        (SHOW_THEMES, SHOW_THEMES_HELP),
        (THEME_EDITOR, THEME_EDITOR_HELP),
    ]),
    ("Taking the result elsewhere", &[
        (OUTPUT, OUTPUT_HELP),
        (LOG, LOG_HELP),
    ]),
    ("Comparing with earlier runs", &[
        (COMPARE_LEVEL, COMPARE_LEVEL_HELP),
        (DIFF, DIFF_HELP),
    ]),
    ("Your data directory", &[
        (SAVE, SAVE_HELP),
        (LOAD, LOAD_HELP),
        (SAVE_THEME, SAVE_THEME_HELP),
        (SHOW_CONFIGS, SHOW_CONFIGS_HELP),
        (RESTORE, RESTORE_HELP),
    ]),
    ("The settings of a project", &[
        (SAVE_LOCAL, SAVE_LOCAL_HELP),
        (NO_LOCAL, NO_LOCAL_HELP),
    ]),
    ("Tuning and diagnostics", &[
        (EXPLAIN, EXPLAIN_HELP),
        (THREADS, THREADS_HELP),
        (SHOW_FAULTY_FILES, SHOW_FAULTY_FILES_HELP),
    ]),
    ("The program itself", &[
        (HELP, HELP_HELP),
        (VERSION, VERSION_HELP),
        (CHANGELOG, CHANGELOG_HELP),
    ]),
];

// The library's errors as this program says them: its colors, broken to a readable width.
pub trait Formatted {
    fn format(&self) -> ColoredString;
}

impl Formatted for RunError {
    fn format(&self) -> ColoredString {
        crate::theme::get_active().warning.paint(&wrap_message(&self.to_string()))
    }
}

impl Formatted for LanguageDirParseError {
    fn format(&self) -> ColoredString {
        wrap_message(&format!("Error: {self}")).red()
    }
}

// Broken between words and never inside one. The lines the message wrote itself are kept, so a
// message that laid itself out is not laid out twice.
pub fn wrap_message(message: &str) -> String {
    message.split('\n').map(wrap_one_line).collect::<Vec<_>>().join("\n")
}

fn wrap_one_line(line: &str) -> String {
    let mut wrapped = String::with_capacity(line.len());
    let mut column = 0;
    for word in line.split(' ') {
        if column > 0 && column + 1 + word.chars().count() > MESSAGE_WIDTH {
            wrapped.push('\n');
            column = 0;
        } else if column > 0 {
            wrapped.push(' ');
            column += 1;
        }
        wrapped.push_str(word);
        column += word.chars().count();
    }

    wrapped
}

// Used both by the full help and by the test that writes the README's command list, so the two
// cannot describe the same commands differently or in a different order. Each summary is the second
// line of that command's own help text.
pub fn create_help_list() -> String {
    let widest = get_command_names().iter().map(|name| name.len() + 2).max().unwrap_or(0);
    let indent = LIST_INDENT + widest + 2;
    let mut list = String::with_capacity(4_000);
    for (group, commands) in COMMAND_HELP {
        list.push_str(&format!("{}\n\n", group.to_uppercase()));
        for (name, help) in commands {
            let summary = help.lines().nth(1).unwrap_or_default().trim();
            let named = format!("{:<width$}", format!("--{name}"), width = widest);
            list.push_str(&format!("{}{named}  {}\n", " ".repeat(LIST_INDENT),
                    hang_under(summary, MESSAGE_WIDTH - indent, indent)));
        }
        list.push('\n');
    }

    list
}

fn hang_under(text: &str, width: usize, indent: usize) -> String {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("a line was pushed before the loop");
        match line.is_empty() {
            true => line.push_str(word),
            false if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            false => lines.push(word.to_owned()),
        }
    }

    lines.join(&format!("\n{}", " ".repeat(indent)))
}

pub fn create_help_body() -> String {
    let mut body = String::with_capacity(20_000);
    for (group, commands) in COMMAND_HELP {
        body.push_str(&group.to_uppercase());
        body.push_str("\n\n");
        for (_, help) in commands {
            body.push_str(help);
        }
    }

    body
}

// The release date lives in the Changelog's first line, 'v3.0.0 - unreleased', and nowhere else. A
// test in this module keeps that line and VERSION_ID together.
pub fn print_version() {
    let changelog = String::from_utf8_lossy(CHANGELOG_BYTES);
    let released = changelog.lines().next().unwrap_or_default().split_once(" - ")
            .map_or_else(|| "unreleased".to_owned(), |(_, date)| date.trim().to_owned());

    println!("
{} ({released})
", super::theme::get_active().version.paint(VERSION_ID));
}

pub fn get_command_names() -> Vec<&'static str> {
    COMMAND_HELP.iter().flat_map(|(_, commands)| commands.iter().map(|(name, _)| *name)).collect()
}

// Painted where it is printed and never where it is built, so create_help_body stays plain text
// and the README generated from it is unaffected.
fn paint_the_help(text: &str) -> String {
    let lines = text.lines().map(|line| {
        if line.is_empty() {
            return line.to_owned();
        }
        if line.starts_with("--") {
            return line.truecolor(HELP_COMMAND.0, HELP_COMMAND.1, HELP_COMMAND.2).bold().to_string();
        }
        if !line.starts_with(' ') && line == line.to_uppercase() {
            return line.bold().to_string();
        }
        paint_the_commands_named_in(line)
    });

    lines.collect::<Vec<String>>().join("\n")
}

// Walked by character because a command is written inside quotation marks, '--save', and splitting
// on spaces would hand back the quotes stuck to the name.
fn paint_the_commands_named_in(line: &str) -> String {
    let mut painted = String::new();
    let mut plain = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '-' || chars.peek() != Some(&'-') {
            plain.push(ch);
            continue;
        }
        let mut named = String::from(ch);
        while chars.peek().is_some_and(|ch| ch.is_ascii_alphanumeric() || "-_".contains(*ch)) {
            named.extend(chars.next());
        }
        painted += &paint_the_value_names_in(&plain, painted.is_empty());
        plain.clear();
        painted += &named.truecolor(HELP_COMMAND.0, HELP_COMMAND.1, HELP_COMMAND.2).to_string();
    }

    let starts_the_line = painted.is_empty();

    painted + &paint_the_value_names_in(&plain, starts_the_line)
}

// 'starts_the_line' is false for the stretch after a command name, since a value is only ever
// written at the head of its line.
fn paint_the_value_names_in(text: &str, starts_the_line: bool) -> String {
    let runs = split_into_runs(text);
    let marked = find_the_value_names_in(&runs, starts_the_line);
    let faded = |text: &str| text.truecolor(HELP_TEXT.0, HELP_TEXT.1, HELP_TEXT.2).to_string();
    let (mut painted, mut plain) = (String::new(), String::new());
    for (at, run) in runs.iter().enumerate() {
        if marked.contains(&at) {
            painted += &faded(&plain);
            plain.clear();
            painted += &run.truecolor(HELP_VALUE_NAME.0, HELP_VALUE_NAME.1, HELP_VALUE_NAME.2).to_string();
        } else {
            plain += run;
        }
    }

    painted + &faded(&plain)
}

// Every list the help draws puts one value at the head of a line, indented into the list and with
// the gap to its description after it: '--style', '--hide', '--layout', '--bar-thickness',
// '--number-separator' and '--progress-bar' are all written that way. Taking the head of the line
// and nothing else is what keeps a description out of it, since the line explaining 'warning' says
// only "warnings" and the one explaining 'version' says "the version line at the top".
//
// The style tokens are the one list written several to a line, so past the head of the line only a
// name from that list is taken.
fn find_the_value_names_in(runs: &[&str], starts_the_line: bool) -> Vec<usize> {
    let is_a_column_edge = |run: Option<&&str>| run.is_none_or(|neighbour| neighbour.len() > 1);
    let head_of_the_line = runs.iter().position(|run| !run.trim().is_empty());
    let is_indented_into_a_list = starts_the_line
            && runs.first().is_some_and(|indent| indent.trim().is_empty() && indent.len() == VALUE_LIST_INDENT);

    (0..runs.len()).filter(|at| is_a_column_edge(at.checked_sub(1).and_then(|before| runs.get(before)))
            && is_a_column_edge(runs.get(at + 1))
            && ((is_indented_into_a_list && head_of_the_line == Some(*at))
                    || crate::theme::Theme::get_token_names().contains(&runs[*at]))).collect()
}

// Alternating stretches of whitespace and of everything else, so that a word can be asked what
// stands on each side of it
fn split_into_runs(text: &str) -> Vec<&str> {
    let mut runs = Vec::new();
    let (mut start, mut in_spaces) = (0, text.starts_with(char::is_whitespace));
    for (at, character) in text.char_indices() {
        if character.is_whitespace() != in_spaces {
            runs.push(&text[start..at]);
            (start, in_spaces) = (at, !in_spaces);
        }
    }
    runs.push(&text[start..]);

    runs
}

pub fn print_whole_help_message() {
    print_the_help(&create_help_body());
}

pub fn print_the_command_list() {
    print_the_help(&format!("{}Run '--help <command>' for the full help of one, or '--help full' \
                             for all of them.\n", create_help_list()));
}

fn print_the_help(body: &str) {
    let mut msg ="
    │  ╲     ╱  ╲
    │ $$╲   ╱  $$  ______   ________  __    __   ______   ______
    │ $$$╲ ╱  $$$ ╱      ╲ │        ╲│  ╲  │  ╲ ╱      ╲ │      ╲
    │ $$$$╲  $$$$│  $$$$$$╲ ╲$$$$$$$$│ $$  │ $$│  $$$$$$╲ ╲$$$$$$╲
    │ $$╲$$ $$ $$│ $$    $$  ╱    $$ │ $$  │ $$│ $$   ╲$$╱      $$
    │ $$ ╲$$$│ $$│ $$$$$$$$ ╱  $$$$_ │ $$__╱ $$│ $$     │  $$$$$$$
    │ $$  ╲$ │ $$ ╲$$     ╲│  $$    ╲ ╲$$    $$│ $$      ╲$$    $$
     ╲$$      ╲$$  ╲$$$$$$$ ╲$$$$$$$$  ╲$$$$$$  ╲$$       ╲$$$$$$$\n\n".to_owned();

    msg += get_data_dir_str().as_str();
    msg += "Format of arguments: <path_here> --optional_command1 --optional_commandN\n\n";
    msg += &paint_the_help(body);

    println!("{msg}");
}

pub fn print_help_message_for_given_args(args_line: &str) {
    let options = crate::args::split_into_command_segments(args_line).into_iter().skip(1).collect::<Vec<_>>();
    if options.len() == 1 {
        match options[0].split_whitespace().nth(1) {
            Some("full") => print_whole_help_message(),
            _ => print_the_command_list(),
        }
        return;
    }

    // The first '--help' is the command being run and not something it was asked about, so it is
    // skipped once. Past that a name is answered once, however many times it was typed.
    let mut command_itself_skipped = false;
    let mut asked : Vec<&str> = Vec::new();
    for option in options {
        let Some(name) = option.split_whitespace().next() else { continue };
        if name == HELP && !command_itself_skipped {
            command_itself_skipped = true;
            continue;
        }
        if !asked.contains(&name) {
            asked.push(name);
        }
    }

    let mut entries = String::new();
    for name in asked {
        match get_help_msg_of_command(name) {
            Some(x) => entries += &paint_the_help(x),
            // The same error the program gives without '--help', so an unknown command does not
            // read as an ordinary line of help text
            None => entries += &format!("{}\n\n", ArgParsingError::UnrecognisedCommand(name.to_owned()).format())
        }
    }

    // The entries and not the whole message: the data dir line is always there, so the message is
    // never empty.
    if entries.is_empty() {
        print_the_command_list();
    } else {
        println!("{}{entries}", get_data_dir_str());
    }
}

// On the error output: every caller is a path that returns a failure, and on stdout this text ends
// up inside a redirected '--output json > stats.json' instead of a document.
pub fn print_help_message_for_command(arg: &str) {
    if let Some(x) = get_help_msg_of_command(arg) {
        eprintln!("\n{}", paint_the_help(x));
    }
}

pub fn print_changelog(full: bool) {
    let changelog = String::from_utf8_lossy(CHANGELOG_BYTES);
    if full {
        println!("\n{changelog}\n");
    } else {
        let latest = changelog.split("-----").next().unwrap().trim_end();
        println!("\n{latest}\n\n(run with '--changelog full' to see the full version history)\n");
    }
}

// The sample includes a real details row, the densest line the program prints: a theme is 46 tokens
// and a mock overview line shows four of them.
pub fn print_existing_themes(bar_thickness: BarThickness, layout: Layout, counting: CountingModel) {
    // Five entries, so that every language slot including the fold gets to show itself. The
    // verticals add up to the width of a real bar.
    // The cells add up to NUM_OF_VERTICALS, so a theme is judged on a bar of the length a run draws
    const MOCK_PERCENTAGES : [(&str, f64, usize); 5] =
            [("first", 40.0, 18), ("second", 26.0, 12), ("third", 16.0, 7), ("fourth", 10.0, 5), ("others", 8.0, 4)];
    const INDENT : &str = "     ";
    // Puts the bar under the first percentage, past the width of the "Lines:" label and its gap
    const BAR_INDENT : usize = 9;

    let mut theme_names = Vec::with_capacity(10);
    let Ok(themes_dir) = fs::read_dir(&PERSISTENT_APP_PATHS.themes_dir) else {
        println!("{}","Could not read the themes dir".yellow());
        return;
    };
    for path in themes_dir.flatten() {
        if let Ok(f) = path.file_type() && f.is_file()
            && let Some(stem) = path.path().file_stem().and_then(|x| x.to_str()) {
            theme_names.push(stem.to_owned());
        }
    }
    theme_names.sort_by_key(|x| x.to_lowercase());

    let mut msg = get_data_dir_str();
    msg.push_str("Found these themes:\n");
    for (at, name) in theme_names.iter().enumerate() {
        // One blank line more between two themes than any block leaves inside one
        if at > 0 {
            msg.push('\n');
        }
        msg.push_str(&format!("\n  {}\n\n", name.bold()));

        let Some((styles, _)) = super::theme_files::load_theme(name, &PERSISTENT_APP_PATHS.themes_dir) else {
            msg.push_str(&format!("{INDENT}{}\n","(this theme could not be read)".yellow()));
            continue;
        };
        let theme = super::theme::resolve(&styles, &[], &[]);

        msg.push_str(&format!("{INDENT}{}.\n", theme.heading.paint("Details")));
        for row in super::result_printer::create_theme_sample_rows(&theme, layout, counting) {
            msg.push_str(&format!("{INDENT}{row}\n"));
        }

        msg.push_str(&format!("\n{INDENT}{}.\n", theme.heading.paint("Overview")));
        msg.push_str(&format!("{INDENT}{}   ", theme.overview_label.paint("Lines:")));
        let slots = theme.get_language_slots();
        for (i, (lang, percentage, _)) in MOCK_PERCENTAGES.iter().enumerate() {
            let text = crate::number_formatter::get_active().percent(*percentage) + "%";
            msg.push_str(&format!("{:>5} {}", theme.overview_percent.paint(&text), slots[i].paint(lang)));
            if i < MOCK_PERCENTAGES.len()-1 {msg.push_str("   ")}
        }

        // On its own line: five language slots plus the bar do not fit next to each other
        msg.push_str(&format!("\n{INDENT}{}{}", " ".repeat(BAR_INDENT), theme.bar_frame.paint("[")));
        for (i, (_, _, verticals)) in MOCK_PERCENTAGES.iter().enumerate() {
            let cell = bar_thickness.get_character().repeat(*verticals);
            msg.push_str(&match slots[i].get_color() {
                Some(color) => cell.color(color).to_string(),
                None => cell
            });
        }
        msg.push_str(&format!("{}\n", theme.bar_frame.paint("]")));
    }

    println!("{msg}");
}

pub fn print_supported_languages(languages_available: &[Language]) {
    println!("{}", format_supported_languages_message(languages_available));
}

pub fn print_existing_configs() {
    let Ok(config_dir) = fs::read_dir(&PERSISTENT_APP_PATHS.config_dir) else {
        println!("{}","Could not read the config dir".yellow());
        return;
    };
    let mut config_names = config_dir.flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            // Skipped rather than shown lossily: this list exists to be typed back into '--load'
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name != "default.txt")
            .collect::<Vec<_>>();
    config_names.sort_unstable();
    println!("{}", format_existing_configs_message(&config_names));
}

// The reason travels beside each name: one heading over both reasons a file can fail is true of
// only one of them, and a file saved in an unreadable encoding would be announced as a file with a
// typo in it.
pub fn format_faulty_language_files_message(faulty_files: &[FaultyLanguageFile]) -> String {
    let mut message = format!("\n{} language {} could not be used, and will not be taken into consideration.",
            faulty_files.len(), if faulty_files.len() == 1 {"file"} else {"files"});
    for faulty in faulty_files {
        message += &format!("\n-- {}: {}", faulty.file_name, faulty.error);
    }
    message + "\n"
}

// Two files declaring one language is a broken installation, and naming it twice here would read as
// two languages rather than as the one it is.
fn format_supported_languages_message(languages_available: &[Language]) -> String {
    const COLUMNS : usize = 3;

    let mut lang_names = languages_available.iter().map(|x| x.name.to_owned()).collect::<Vec<_>>();
    lang_names.sort();
    lang_names.dedup();
    format!("{}Found these languages:\n\n{}\n", get_data_dir_str(),
            format_in_columns(&lang_names, COLUMNS))
}

// Filled downwards and not across, so a sorted list reads in order down each column.
fn format_in_columns(names: &[String], columns: usize) -> String {
    const GUTTER : usize = 6;

    let rows = names.len().div_ceil(columns);
    let width = names.iter().map(|name| name.chars().count()).max().unwrap_or(0) + GUTTER;
    (0..rows).map(|row| {
        let line = (0..columns).filter_map(|column| names.get(column * rows + row))
                .map(|name| format!("{name:<width$}")).collect::<String>();
        format!("  {}", line.trim_end())
    }).collect::<Vec<_>>().join("\n")
}

// The empty case gets a sentence of its own: joining no names with the two spaces that indent the
// first one leaves a heading over a line of two spaces, which reads as a name that failed to print.
fn format_existing_configs_message(config_names: &[String]) -> String {
    if config_names.is_empty() {
        format!("{}No configurations found.\n", get_data_dir_str())
    } else {
        format!("{}Found these configurations:\n  {}\n", get_data_dir_str(), config_names.join("\n  "))
    }
}

fn get_data_dir_str() -> String {
    format!("\nData directory: {}\n\n", PERSISTENT_APP_PATHS.data_dir.trim_end_matches(['/', '\\']))
}

fn get_help_msg_of_command(command: &str) -> Option<&'static str> {
    COMMAND_HELP.iter().flat_map(|(_, commands)| commands.iter())
            .find(|(name, _)| *name == command).map(|(_, help)| *help)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_wraps_between_words_and_keeps_the_breaks_it_had() {
        let short = "'x' is not a valid glob pattern.";
        assert_eq!(short, wrap_message(short));

        let long = "Everything that the pattern 'services/*' matched is skipped, because a .gitignore file \
                ignores it, because it is a dotted path, or because it is a link.\nUse the '--no-gitignore' \
                or '--search-in-dotted' commands to include it, or provide the paths explicitly.";
        let wrapped = wrap_message(long);
        for line in wrapped.lines() {
            assert!(line.chars().count() <= MESSAGE_WIDTH, "'{line}' is {} columns", line.chars().count());
        }
        assert_eq!(long.split_whitespace().collect::<Vec<_>>(), wrapped.split_whitespace().collect::<Vec<_>>());
        assert!(wrapped.lines().any(|x| x.starts_with("Use the '--no-gitignore'")));

        // a word longer than the width has nowhere to break
        let path = "a/".repeat(MESSAGE_WIDTH);
        assert_eq!(format!("see\n{path}"), wrap_message(&format!("see {path}")));
    }

    #[test]
    fn a_language_declared_by_two_files_is_listed_once() {
        let none = mezura_core::StringRules::escaping_nothing;
        let twice = vec![Language::new("Java", ["java"], none(), ["//"], &[], []),
                Language::new("Rust", ["rs"], none(), ["//"], &[], []),
                Language::new("Java", ["jav"], none(), ["//"], &[], [])];
        let listed = format_supported_languages_message(&twice);

        assert_eq!(1, listed.matches("Java").count(), "'Java' was listed more than once:\n{listed}");
        assert!(listed.contains("Rust"));
    }

    // The columns are filled downwards, so the last one is short whenever the count misses a
    // multiple of three, and that ragged end is where a name goes missing.
    #[test]
    fn every_name_survives_a_column_count_that_does_not_divide_the_list() {
        let all = (0..10).map(|i| format!("Lang{i}")).collect::<Vec<_>>();
        for count in 0..=all.len() {
            let names = &all[..count];
            let laid_out = format_in_columns(names, 3);
            for name in names {
                assert_eq!(1, laid_out.matches(name.as_str()).count(),
                        "'{name}' is not listed exactly once among {count} names:\n{laid_out}");
            }
            assert!(laid_out.lines().all(|line| line.len() == line.trim_end().len()),
                    "a line was left padded to the right:\n{laid_out}");
        }
    }

    // The one message that only appears when something in the user's languages folder is wrong, so
    // no ordinary run prints it and no comparison of two builds can see it.
    #[test]
    fn a_language_file_that_could_not_be_read_is_not_called_a_formatting_problem() {
        let unreadable = FaultyLanguageFile {
            file_name: "Utf16.txt".to_owned(),
            error: mezura_core::language_file::LanguageFileError::Unreadable(
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "stream did not contain valid UTF-8"))
        };
        let malformed = FaultyLanguageFile {
            file_name: "Garbage.txt".to_owned(),
            error: mezura_core::language_file::LanguageFileError::Malformed(7)
        };

        let both = format_faulty_language_files_message(&[unreadable, malformed]);
        assert!(both.contains("2 language files could not be used"), "{both}");
        assert!(both.contains("-- Utf16.txt: the language file could not be read"), "{both}");
        assert!(both.contains("-- Garbage.txt: line 7 is not what the format expects there"), "{both}");
        assert!(!both.contains("Formatting problems"), "the two reasons are under one wrong heading again:\n{both}");

        let one = format_faulty_language_files_message(&[FaultyLanguageFile {
            file_name: "Garbage.txt".to_owned(),
            error: mezura_core::language_file::LanguageFileError::Malformed(1)}]);
        assert!(one.contains("1 language file could not be used"), "{one}");
    }

    #[test]
    fn an_empty_config_dir_says_so_instead_of_listing_nothing() {
        let none = format_existing_configs_message(&[]);
        assert!(none.contains("No configurations found."), "{none}");
        assert!(!none.contains("Found these configurations"), "{none}");
        assert!(!none.contains("\n  \n"), "an empty bullet was printed:\n{none}");

        let some = format_existing_configs_message(&["mezura.txt".to_owned(), "portal.txt".to_owned()]);
        assert!(some.contains("Found these configurations:\n  mezura.txt\n  portal.txt"), "{some}");
        assert!(none.contains("Data directory:") && some.contains("Data directory:"));
    }

    const README_HEADING : &str = "### Commands";
    const FENCE : &str = "```";

    // The middle part alone is replaceable, so the rest of a hand written document is never touched.
    fn readme_parts(readme: &str) -> (String, String, String) {
        let heading_at = readme.find(README_HEADING).expect("the README has a '## Cmd Commands' heading");
        let opening = readme[heading_at..].find(FENCE).expect("that section opens a fenced block") + heading_at;
        let body_at = opening + FENCE.len();
        let closing = readme[body_at..].find(FENCE).expect("that fenced block is closed") + body_at;

        (readme[..body_at].to_owned(), readme[body_at..closing].to_owned(), readme[closing..].to_owned())
    }

    // The README's command list is not maintained, it is written from the help texts. Anything the
    // README wants to say that the help does not has to live outside the fence, since the inside of
    // it is replaced wholesale.
    #[test]
    fn the_readme_command_list_is_the_help_itself() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("README.md");
        let readme = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let (before, block, after) = readme_parts(&readme);
        let generated = create_help_list();

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, format!("{before}\n{}\n{after}", generated.trim_end())).unwrap();
            return;
        }

        assert_eq!(block.trim(), generated.trim(),
                "the command list in the README no longer matches the help texts, which are the source. \
                 Regenerate it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme");
    }

    fn create_commands_document() -> String {
        let mut document = String::with_capacity(60_000);
        document.push_str("# Commands\n\nThe full help of every command, exactly as \
`mezura --help <command>` prints it. A test writes this file from the help texts themselves, \
so do not edit it by hand. Regenerate it with `MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura \
commands_document`.\n\n");
        for (group, commands) in COMMAND_HELP {
            let anchor = group.to_lowercase().replace(' ', "-");
            document.push_str(&format!("- [{group}](#{anchor})\n"));
            for (name, _) in commands {
                document.push_str(&format!("  - [--{name}](#cmd-{name})\n"));
            }
        }
        for (group, commands) in COMMAND_HELP {
            document.push_str(&format!("\n## {group}\n"));
            for (name, help) in commands {
                document.push_str(&format!("\n### <a id=\"cmd-{name}\" name=\"cmd-{name}\"></a>--{name}\n\n```\n{}\n```\n",
                        help.trim_matches('\n')));
            }
        }

        document
    }

    // The whole document is written from the help texts, so anything it should say that the help
    // does not belongs in the README instead.
    #[test]
    fn the_commands_document_is_the_help_itself() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("COMMANDS.md");
        let generated = create_commands_document();

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &generated).unwrap();
            return;
        }

        let document = std::fs::read_to_string(&path)
                .expect("COMMANDS.md is missing; write it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura commands_document")
                .replace("\r\n", "\n");
        assert_eq!(document, generated,
                "COMMANDS.md no longer matches the help texts, which are the source. \
                 Regenerate it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura commands_document");
    }

    // Asserted on the rule and not on the painted string, because whether anything is painted at
    // all is a process-wide switch that another test in this binary turns off.
    #[test]
    fn a_value_is_marked_where_it_names_itself_and_not_where_a_sentence_uses_the_word() {
        fn marked(line: &str) -> Vec<&str> {
            let runs = split_into_runs(line);
            find_the_value_names_in(&runs, true).into_iter().map(|at| runs[at]).collect()
        }

        // The word is in the line twice and only the one at the head of it is the value
        assert_eq!(vec!["version"], marked("      version                  the version line at the top"));
        // A description of one word is still a description
        assert_eq!(vec!["warning"], marked("      warning                  warnings"));
        // Values of a command that has nothing to do with the styling, and one whose description
        // opens with a character rather than a word
        assert_eq!(vec!["matrix"], marked("      matrix    languages down, modules across, one number per cell"));
        assert_eq!(vec!["slim"], marked("      slim     |   plain ASCII, so it renders on any terminal"));
        // Several to a line, as the table of style tokens is written
        assert_eq!(vec!["files-number", "files-label", "comments-number", "comments-label"],
                marked("      files-number  files-label             comments-number  comments-label"));

        // Not indented into a list, so the head of the line is text like any other
        assert!(marked("    A style is one or two colors and any number of the attributes").is_empty());
        assert!(marked("        mezura frontend=./web backend=./api").is_empty());
        // Indented into one, and named after the command it belongs to rather than after a value
        assert_eq!(vec!["nothing-of-ours"], marked("      nothing-of-ours          is checked past the head of the line"));
    }

    // The help is the only place that lists these, and the README is generated from it. Asked as a
    // column rather than as a substring: 'version' also sits inside "the version line at the top",
    // so a plain search passes for a name nobody ever listed. It is the same question the painter
    // asks, so this is also what keeps a name from being reflowed out of its column and quietly
    // printing unpainted.
    #[test]
    fn every_value_of_the_two_commands_that_list_them_is_listed_in_a_column() {
        let listed_in = |help: &str, name: &str| help.lines().any(|line| {
            let runs = split_into_runs(line);
            find_the_value_names_in(&runs, true).into_iter().any(|at| runs[at] == name)
        });

        for token in crate::theme::Theme::get_token_names() {
            assert!(listed_in(STYLE_HELP, token), "'{token}' is a style token that the help never lists \
                    in a column of its own, so it is neither found by somebody reading the list nor painted in it");
        }
        for name in crate::config_manager::Hidden::get_names() {
            assert!(listed_in(HIDE_HELP, name), "'{name}' is something '--hide' accepts that its help never \
                    lists in a column of its own, so it is neither found by somebody reading the list nor painted in it");
        }
    }

    // The other four commands that list their values have no name list to check against, so the
    // help's own two copies are checked against each other and both against the parser: the
    // sentence that opens 'One argument:' has to name exactly what the columns below it name, and
    // the command has to accept every one of them. A value reflowed out of its column would
    // otherwise keep printing, plain among painted siblings, with nothing failing.
    #[test]
    fn each_command_that_lists_its_values_paints_the_ones_it_accepts_and_no_others() {
        fn check(command: &str, help: &str, accepts: impl Fn(&str) -> bool) {
            let mut in_a_column = help.lines().flat_map(|line| {
                let runs = split_into_runs(line);
                find_the_value_names_in(&runs, true).into_iter().map(|at| runs[at].to_owned())
                        .collect::<Vec<_>>()
            }).collect::<Vec<_>>();
            let mut in_the_sentence = help.lines()
                    .find(|line| line.trim_start().starts_with("One argument:")).unwrap_or_default()
                    .split('\'').skip(1).step_by(2).map(str::to_owned).collect::<Vec<_>>();
            in_a_column.sort();
            in_the_sentence.sort();

            assert_eq!(in_the_sentence, in_a_column, "the values '--{command}' names in its opening \
                    sentence are not the ones its help lists in a column, so one of the two is wrong \
                    and the columns are what gets painted");
            for value in &in_a_column {
                assert!(accepts(value), "'--{command}' does not accept '{value}', which its help lists \
                        as one of its values");
            }
        }

        check(LAYOUT, LAYOUT_HELP, |x| Layout::parse(x).is_some());
        check(BAR_THICKNESS, BAR_THICKNESS_HELP, |x| BarThickness::parse(x).is_some());
        check(NUMBER_SEPARATOR, NUMBER_SEPARATOR_HELP, |x| NumberSeparator::parse(x).is_some());
        check(PROGRESS_BAR, PROGRESS_BAR_HELP, |x| ProgressBarStyle::parse(x).is_some());
    }

    // '--version' reads the release date from the first line of the Changelog, so without this the
    // two drift apart silently and '--version' quotes the date of a release that is not running.
    #[test]
    fn the_changelog_opens_with_the_version_this_binary_reports() {
        let changelog = String::from_utf8_lossy(CHANGELOG_BYTES);
        let first = changelog.lines().next().unwrap();
        assert!(first.starts_with(&format!("{VERSION_ID} - ")),
                "the Changelog opens with '{first}', which does not start with '{VERSION_ID} - '");
    }

    // The list a bare '--help' prints is these second lines, so a command without one is a blank
    // next to its name there.
    #[test]
    fn every_command_opens_its_help_with_a_summary_of_itself() {
        for name in get_command_names() {
            let help = get_help_msg_of_command(name).expect("every command has a help entry");
            let summary = help.lines().nth(1).unwrap_or_default().trim();
            assert!(!summary.is_empty(), "'--{name}' says nothing on the second line of its help");
            assert!(!summary.starts_with("--"), "'--{name}' opens with a command and not a summary");
        }
    }

    #[test]
    fn every_command_has_exactly_one_help_entry() {
        let names = get_command_names();
        for name in &names {
            assert!(get_help_msg_of_command(name).is_some(), "'--{name}' has no help entry");
            assert_eq!(1, names.iter().filter(|x| *x == name).count(), "'--{name}' is listed twice");
        }
    }
}
