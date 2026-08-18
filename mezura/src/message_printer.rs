use std::fs;

use colored::{ColoredString, Colorize};
use mezura_core::{CountingModel, Language, RunError};
use mezura_core::language_file::{FaultyLanguageFile, LanguageDirParseError};

use super::config_manager::*;
use crate::paths::PERSISTENT_APP_PATHS;

// The file itself, so that the command never depends on an installation having a copy of it.
static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");
// Wide enough for a sentence to read as one, narrow enough that a terminal is unlikely to break it
// somewhere of its own choosing, which is what a message wider than the window looks like
const MESSAGE_WIDTH : usize = 110;

// These constants need to be maintained along with the readme's commands
pub const TARGETS_HELP  :  &str =
"--targets

    The paths to the directories or files, separated by commas if more than 1,
    in this form: '--targets <path1>, <path2>'
    A path can also be a glob pattern (* ? [..] {..}), which is expanded to every existing
    directory and file that it matches, so 'services/*/src' is a valid target.
    A path that exists exactly as written is always taken literally, so a folder with one of
    those characters in its name is still just a folder.
    Since the matches of a pattern are found by the program and not named by you, they follow
    the same rules as every other path it discovers: the ones that a .gitignore ignores, that
    are dotted, or that are links, are skipped (see the '--no-gitignore' and '--search-in-dotted'
    commands).
    A path that you write out explicitly is always used, even if it is ignored, dotted or a link.
    Targets that are contained in other targets are dropped, so that no file is counted twice.
    If you are using Windows Powershell, you will need to escape the commas with a backtick: `
    or surround all the arguments with quotation marks:
    <path1>`, <path2>`, <path3>   or   \"<path1>, <path2>, <path3>\"

    MODULES

    A target can be given a name, and then the report is grouped by it as well as by language,
    which answers 'how much of this is the frontend' without running the program once per folder:

        mezura frontend=./web backend=./api
        mezura ./project tests=./project/tests

    A comma continues the list of paths of the same module and a space ends it, so
    'tests=./api/tests,./web/tests' is one module of two directories, while 'frontend=./web ./ui'
    is the module and a separate unnamed target. Repeating a name adds to it.
    Every file belongs to exactly one module, and the most specific path wins, so the second
    example above means 'the tests there, the rest of the project here', whichever order they are
    written in. Anything the named ones did not claim is one row called '(unnamed)', and it comes
    last. The rest appear in the order you wrote them, which is how you arrange the columns of the
    'matrix' layout: '--sort' orders the languages inside a module and never the modules themselves.
    Declaring the same path under two different names is refused, since there is nothing more
    specific to settle it. A run that names nothing prints exactly what it always did.

    A space only ends a target once you have named one. While no name is given, a path is allowed
    to contain spaces and only a comma separates two of them, which is the way it has always been.
    That does leave one thing a command line cannot say, a path with a space in it in a run that
    also names modules, because your shell removes the quotation marks before the program sees
    them. Write those in a configuration file, one target per line, where a space never separates.

    The target directories can also be given implicitly (in which case this command is not needed) with 2 ways:
    1) as the first arguments of the program directly
    2) if they are present in a configuration file (see '--save' and '--load' commands).

";
pub const EXCLUDE_HELP  :  &str =
"--exclude

    1..n glob patterns separated by commas.

    A pattern without a slash matches a file or folder name at any depth ('node_modules', '*.min.js').
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

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    By default, the program respects .gitignore files: any file or folder ignored by a .gitignore
    found in the traversed directories (or in their parent directories, up to the repository root)
    is skipped, and skipped files are included in the excluded files count. Negated patterns
    ('!keep.log') are supported, and target paths that are written out explicitly are always used,
    even if a .gitignore of their parent directories would ignore them. The matches of a glob
    pattern do not count as written out explicitly, since the program is the one that found them.

    This flag disables that behavior, so that every relevant file is counted
    regardless of .gitignore rules.

";
pub const LANGUAGES_HELP  :  &str =
"--languages

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' dir gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    An extension two languages claim names whichever of them owns it for the counting, which is
    the answer in 'extension_priority.txt' or the one '--force-language' gave.

    Only the languages specified here will be taken into account for the stats.

";
pub const EXCLUDE_LANGUAGES_HELP  :  &str =
"--exclude-languages

    1..n arguments separated by commas, case-insensitive

    A language is named either by the name a file in the 'data/languages/' dir gives it under
    'Language', or by any extension it claims, so 'javascript' and 'js' name the same one.

    The given language names will be ignored from the stats calculation, if they exist.

";
pub const FORCE_LANGUAGE_HELP  :  &str =
"--force-language

    1..n pairs of 'extension=language' or 'filename=language' separated by commas, case-insensitive

    Decides which language an extension is counted as, whether or not another language claims it:
    '--force-language m=matlab,pl=perl,txt=python'

    A whole filename works the same way, for the files that have no extension worth reading:
    '--force-language Makefile=python,Jenkinsfile=groovy'

    Overrides the 'extension_priority.txt' file of the data dir.

";
pub const THREADS_HELP  :  &str =
"--threads

    2 numbers: the first between 1 and 32 and the second between 1 and 128.

    This represents the number of the producers (threads that will traverse the given directories),
    and consumers (threads that will parse whatever files the producers found).

    If this command is not provided, the numbers will be chosen based on the available threads
    on your machine.

    There are far more consumers than cores on purpose. A consumer spends most of its life waiting
    for a file to open, so what decides the speed is how many reads are in flight, not how many
    cores there are. Raising the number costs nothing on a fast disk whose files are already
    cached, and is worth up to twice the speed on a slow disk or on the first run after a reboot.
    If your files live on a slow drive, or you are counting a tree you have not touched today, it
    is worth trying a higher number than the default.

";
pub const COUNTING_HELP  :  &str =
"--counting

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

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Specifies whether the program should traverse directories that are prefixed with a dot,
    like .vscode or .github.

    The '.git' directory is never traversed, with or without this command, at any depth. Nothing
    inside it is source, and walking it is thousands of files for no count at all.

";
pub const EXPLAIN_HELP  :  &str =
"--explain

    No arguments. The target must be exactly one file.

    Explains that file line by line instead of printing a report. Every line is shown with the
    bucket it lands in under the active way of counting, the class mezura itself read off it,
    and, where something was still open when the line began, what that was and where it started:
    'in a comment opened by /* on line 23', 'in a string opened by \" on line 7'. In a file
    holding other languages, a line read by an embedded language names it. The source lines are
    printed with the stretches that sit inside a string or a comment in their own styles, which
    '--style' reaches as 'explain-string' and 'explain-comment', so a symbol swallowed by a
    string is visible as such.

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

";

pub const SHOW_FAULTY_FILES_HELP  :  &str =
"--show-faulty-files

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
    or while reading its contents. A directory can also fail to be opened at all, most often because
    of its permissions, or because something removed it while the scan was running. The default
    behavior in both cases is to count them and display the count, since everything under a directory
    that could not be read is missing from every number in the report.

    This flag specifies that their path, along with information about the exact error is displayed too.
    The most common reason for a faulty file is if it contains non UTF-8 characters.

    It asks the same of '--output json', where the two lists of paths are written only when this
    flag is given. How many there were is in the 'scan' block either way, so a document without
    the lists never claims that nothing went wrong. A comparison document carries the counts of
    each side and no lists at all.

";
pub const HIDE_HELP  :  &str =
"--hide

    One or more names separated by commas or spaces, for example:
    --hide parsing-info,timing   or   --hide parsing-info timing

    Leaves the named parts of the output unprinted. What you can hide:

      version         the version line at the top
      directory-info  the 'Analyzing directories' line and the 'N files found' line under it
      parsing-info    the 'Parsing files' line and the 'ok' under it
      progress-bar    the bar, the share done and the speed figures of a long parse, keeping
                      its file count
      animations      every moving line: the scan's dots, the live progress bar, and the
                      'Writing out', 'Counting' and 'Cleaning up' lines of a '--diff'. What
                      they settle into still prints. A TERM=dumb terminal gets this on its own
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

    The list mixes whole sections with parts of them on purpose: you are pointing at what you
    see, not at how the program is organised.

    The five column names reach the details in every layout except the matrix, whose three rows
    stay whole. A comparison obeys them too, where every change follows its own figure out; its
    percentages are percentages of the change, so hiding them leaves the absolute move, and it has
    no 'extra' column to hide. Hiding the column '--sort' orders by falls back to sorting by
    lines, and says so.

    Errors and warnings are never hidden. Hiding the parsing info still reports files that failed
    to be parsed, since otherwise the numbers would silently be wrong.

    Replaces the '--no-visual' and '--no-keywords' commands of previous versions.

";
pub const THEME_HELP  :  &str =
"--theme

    One argument, the name of a theme (case-insensitive).

    Applies a named theme. Themes are .txt files in the 'data/themes' dir, in the persistent
    data path of the application, where the file name is the theme name. Every line is a
    'token = value' pair, the same tokens and values that '--style' takes (see '--help style').
    You can add your own there, and '--show-themes' lists the ones you have.

    A theme carries only how the output looks. What is measured and what is shown stays in a
    configuration file, so a theme can be handed to someone else without carrying your paths
    or your settings with it.

    A style that does not parse is reported and skipped, and the rest of the theme still applies.
    A name that matches no file is an error, since that one is a mistake in the command.

";
pub const SAVE_THEME_HELP  :  &str =
"--save-theme

    One argument, the name of the theme file to write (case-insensitive, no extension).

    Writes everything about the way this run looks into a theme file: whatever theme was loaded,
    plus the style block of the configuration, plus any '--style' given on the command line, all
    flattened into values. The file stands on its own and can be shared as it is.

    Combined with '--save', the configuration that is written points at this theme by name and
    carries no styles of its own.

";
pub const SORT_HELP  :  &str =
"--sort

    One argument: 'lines', 'files', 'code', 'comments', 'extra', 'blanks', 'size' or 'name'.
    Default: lines
    Every column of the details table is one of them, so there is no figure you can see and not
    order by. The third column is 'extra' under '--counting content' and 'blanks' under
    '--counting region', and naming the other model's word orders by lines and says so.

    Chooses the order of the languages in the \"details\" section, which also decides which of them
    reach the \"overview\" section and which are folded into its 'others' entry.
    Every criterion except 'name' sorts from the largest down, and ties are broken alphabetically
    so that the order never changes between runs on the same data.
    The column that decides it carries a mark in its header, since the criterion can come from a
    configuration file and then nothing else on the page would say it.

    Note that before v3.0.0 the order was a fixed formula in which the byte size dominated, so
    runs that used to be ordered by size are now ordered by lines unless you ask otherwise.

";
pub const TOP_HELP  :  &str =
"--top

    One number, 1 or greater.

    Shows only that many languages in the \"details\" section, the ones that come first according
    to the '--sort' criterion. A line underneath states how many were hidden, so that the numbers
    not adding up to the total is never a mystery.

    The total keeps counting every language, hidden ones included, since it is the total.
    The \"overview\" section never shows more languages than this either, so asking for the top 2
    does not leave a third one sitting in the bar.

    It never reorders the modules themselves, only the languages inside them: you chose that order
    when you wrote them.
    With modules, the cut happens inside each one, since that is what the rows under a module are.
    The 'matrix' layout is the exception: its rows are the languages of the whole run, so there the
    cut is over all of them.

";
pub const BAR_THICKNESS_HELP  :  &str =
"--bar-thickness

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: medium

    Chooses the character that the percentage bar of the \"overview\" section is drawn with.

      slim     |   the only one made of ASCII, so it is the one that is guaranteed to
                   render on every terminal
      medium   ┃   the default, thicker, but still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a terminal and a font that can render box drawing characters.
    If the bar comes out as question marks or empty boxes, use 'slim'.

";
pub const PROGRESS_BAR_HELP  :  &str =
"--progress-bar

    One argument: 'smooth', 'blocky' or 'hash'. Default: smooth

    Chooses the characters that the live progress bar of a long parse is drawn with.

      smooth   ▏▎▍▌▋▊▉█   one unbroken bar, its tip gliding through eight sub-steps per cell
      blocky   ▪▮         separate boxes, each drawn narrower than its cell, so a small gap
                          falls between them
      hash     .:#        the only one made of ASCII, so it is the one that is guaranteed
                          to render on every terminal

    The bar only appears on a terminal, on a parse long enough to watch, with the share done
    beside it; '--hide progress-bar' keeps its file count and drops the rest.

";
pub const LAYOUT_HELP  :  &str =
"--layout

    One argument: 'table', 'boxed', 'list' or 'matrix'. Default: table

    Chooses the shape of the \"details\" section.

      table     one aligned row per language: Language, Files, Lines, Code, Comments, Extra
                and Size, with a percentage next to each of the first four. No borders, only
                whitespace alignment, so it survives being pasted into a README or a ticket.
      boxed     the same figures inside a drawn frame. Each number shares a cell with its
                percentage there, since the borders already group the two, which makes it
                narrower than 'table'. Needs a terminal that can render box drawing
                characters.
      list      one block of three rows per language: the file count and the size above the
                name, the line breakdown beside it, the keywords below. Wider, and it cannot
                be read down a column, but it reads well for a handful of languages.
      matrix    languages down, modules across, one number per cell. The other three answer
                'what is inside the backend', read down a section; this one answers 'how do
                the modules compare on the same language', read along a row, which is what
                you want when the folders you named are several answers to one problem rather
                than several parts of one thing. Only one number fits in a cell, so it holds
                whatever '--sort' is ordering by, and a line above the table says which.
                A dash is a language the module does not have. With no module named there is
                nothing to cross, so it says so and prints 'table' instead.

    The percentage next to 'Files' and to 'Lines' is that language's share of the total, the
    one next to 'Code' and to 'Comments' is its share of that language's own lines, which is
    what the same two numbers mean in the 'list' layout.

    In the two tables the keywords cannot be a column without destroying the alignment, so they
    are printed as their own block underneath, one line per language. '--hide keywords' still
    suppresses them.

";
pub const RESTORE_HELP  :  &str =
"--restore

    No arguments.

    Overrides normal program execution and brings your data directory back to what this version of
    mezura ships: anything missing is written, and a language file that no longer says what ours
    says is replaced. It reports what it did.

    This already happens on its own whenever the mezura you run carries different files from the
    ones your data directory was given, so you should not need it. It is here for when something
    was damaged or deleted while the program itself stayed the same, where nothing else would
    notice.

    A language file you changed is replaced too, since one that has fallen behind counts wrongly,
    but your copy is kept under 'data/replaced/<version>/<date and time>/' and named, so you can
    carry your changes over. Each run of this writes its own folder there, so running it twice never
    mixes the two. A language file of your own is never touched, and neither are your themes or your
    default configuration: those are written when absent and left alone.

    'extension_priority.txt' is neither replaced nor left alone. Every rule you wrote in it is kept,
    and the rules a new version adds are added beside them, so a contest you have already settled
    stays settled while a new one does not go unmentioned. Change who wins a contest by reordering
    the names on its line rather than by deleting the line: a line that is not there reads as one
    you never had, and comes back. Your copy as it stood is kept under 'replaced' whenever anything
    is added to it.

";
pub const OUTPUT_HELP  :  &str =
"--output

    One argument: 'text' or 'json'. Default: text

    Chooses what mezura writes to its output. 'json' replaces everything, the status lines and
    the overview included, with a single JSON document, so that the run can be read by another
    program instead of by a person.

      mezura ./src --output json > stats.json
      mezura ./src --output json | jq '.total.code'

    The document carries the counts as plain numbers of lines and bytes: no thousands
    separators, no KB or MB, no percentages and no colors, whatever the rest of the settings
    say. '--sort' and '--top' still order and cut the list of languages, and the count of the
    ones left out is in the document. '--hide keywords' and '--hide timing' remove what they
    name, since it is either not counted or not measured; the rest of the '--hide' list names
    printed sections that a JSON run does not have.

    Warnings and errors are written to the error output, so no stray line can land inside the
    document, and the warnings are carried in it as well, under 'warnings'. Each one has a
    'code' that is safe to branch on and will not change wording under you, an 'affects' of
    'counts' or 'settings', the 'subject' it is about, and the readable 'message'. Ask whether
    any of them affects the counts to know whether the numbers can be trusted: a language file
    that could not be read means a whole language went uncounted, while an ignored setting does
    not touch a number. The list is there even when it is empty.

    A run that finds nothing to count still writes a valid document, with an empty list of
    languages and a total of zero.

    This is the one display setting that a configuration file cannot carry, so that no saved
    configuration can silently turn the output of every later run into JSON.

";
pub const DIFF_HELP  :  &str =
"--diff

    One argument: a reading, or two of them with '..' between, oldest first. A reading is the
    path of a JSON document that an earlier run wrote, or a git revision: a branch, a tag, or
    enough of a commit hash to be unique.

    Compares this run against that reading, or the two readings against each other. The
    comparison takes the place of the report rather than sitting under it, so no language is
    listed twice.

      mezura ./src --output json > baseline.json
      mezura ./src --diff baseline.json
      mezura ./src --diff main
      mezura ./src --diff v2.0.1..v3.0.0
      mezura --diff january.json..june.json

    A revision is counted on the spot, over its own files, with this run's settings and its
    targets: 'mezura ./src --diff main' counts what './src' held on 'main'. The targets must
    all be inside one git repository, and a directory the revision does not have counts as
    zero, so everything in it reads as new. What was found on disk decides which of the two a
    reading is: a name that is a file is a document, anything else is asked of git. A revision
    is any spelling git itself resolves: a branch, a tag, a hash, or a remote-tracking name
    like 'origin/main'. It has to have been fetched already; one that lives only on the remote
    needs a 'git fetch' first, mezura never touches the network.

    Counting a revision costs more than counting the same tree in place: the commit's files
    are first written out whole to a temporary directory, and a file written moments ago is
    slower to read back than one that has sat on disk. The checkout is removed in the
    background after the comparison prints. Size can also move when no line did: git writes
    the checkout with the line endings 'core.autocrlf' asks for, and a working tree saved
    with the other ending then differs by one byte per line, in Size and nowhere else.

    Only one '..' is allowed, and it is the separator. A path that climbs through one on its
    way to a file is taken whole when the file is really there, so '--diff ../old.json' reads
    as you would expect; two of them cannot be told apart from a separator and are refused, so
    write such a path out without the climb.

    Every figure carries what it is now and how much it moved, and the three counted in
    thousands carry the percentage as well; a file count and a size are read whole, so '+2
    files' is the answer there and a decimal point would be the same fact dressed up. A figure
    that did not move is a dash, so the eye goes to the rows that did. The columns that a plain
    run gives to each figure's share of the whole are what the change is written in, and
    'Extra' is gone, being the three columns beside it taken off the lines.

    The keywords are marked the same way, in the section they already have, and only where one
    moved: 'structs: 57 (+5), traits: 2'. The overview is not printed, being a picture of one
    reading, and neither is the history section, which is a comparison of its own against the
    log's entries: two of those under one another answer 'what changed since' twice with
    different pasts.

    '--sort' and '--top' order and cut the rows as they do everywhere else. The comparison is
    drawn as the table, or in the boxed frame with '--layout boxed'; 'list' and 'matrix' have
    nothing to show for one, and mezura says so and prints the table. A language that only one
    of the two readings has is marked 'new' or 'gone' instead of being given a percentage,
    since a count that grew out of nothing has none.

    The modules get a row each, with their languages under them, when the two readings named
    the same ones. When they did not, there is no module the two of them share: the comparison
    is of everything at once, and mezura says which each of them named. A module that one
    reading has and the other does not would have every language in it read as written from
    scratch or deleted whole, for files that in all likelihood only moved.

    A document records the settings the counting obeyed, and they reach whatever is counted
    against it: comparing against a baseline taken with '--exclude target' leaves that directory
    out of this run too, and says so, because the two readings would otherwise have counted
    different trees and that difference would read as code that changed. A setting given
    on this very command line is kept instead, and the comparison then warns that the two
    readings were not taken the same way, as it does when two documents disagree with each
    other and there is nothing left to count. '--counting' travels the same way, so the columns
    read as the baseline's did, but a difference in it is never warned about: a document records
    where every line landed and not one fold of it, so both sides are folded alike whatever they
    were written under. '--no-gitignore' never reaches a revision, and
    mezura says so: a checkout holds only what git tracks. The comparison also says so when
    the two readings were counted by different versions of mezura.

    A document written with '--top' is refused, because the languages it left out would read as
    languages that were deleted since. Write the baseline without it.

    With '--output json' the comparison itself is written as a document: the same vocabulary as
    a run's document, with every count carrying 'from', 'to' and 'change', and the two readings
    identified under 'from' and 'to' by their source, so a build step can ask questions like
    'did the code shrink' of the change directly. '--top' does not cut that document, being a
    decision about a screen.

    This is not a display setting a configuration file can carry, so that no saved
    configuration can silently turn every later run into a comparison.

";
pub const NUMBER_SEPARATOR_HELP  :  &str =
"--number-separator

    One argument: 'comma', 'underscore', 'dot' or 'none'. The character itself is also
    accepted, so '--number-separator _' is the same as '--number-separator underscore'.
    Default: comma

    Chooses the character that groups the digits of every printed number.

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

    One argument: 'dot' or 'comma'. The character itself is also accepted, so
    '--decimal-separator ,' is the same as '--decimal-separator comma'. Default: dot

    Chooses the character that separates the decimals of every printed number: the sizes,
    the percentages and the execution time.

    It is free to be the same character as the one '--number-separator' groups the digits
    with, since both conventions are in use somewhere. Nothing that is written to a log
    file is affected, so a log stays readable by any version.

";
pub const STYLE_HELP  :  &str =
"--style

    One or more 'token=style' pairs separated by commas, for example:
    --style code-number=bright-black,code-label=b5a98a italic,heading=white bold underline

    Overrides how a category of printed text looks. A style is a color and any number of the
    attributes 'bold', 'italic', 'underline', 'dim' and 'reverse', in any order. The color is
    either a hex value or one of the 16 terminal color names (the same grammar as '--colors'),
    or the word 'default' to leave the terminal's own foreground color alone.

    'reverse' swaps the text and background colors, so it stands out strongly without committing
    to any color of its own, which means it adapts to whatever theme the terminal is using.

    The cells of the live progress bar take two forms no other token does, being a run of cells
    rather than one piece of text: two or more hex values separated by '..' fill them with a
    gradient through every one of them, evenly spread, and 'rainbow' walks a spectrum along them
    while they are drawn. A gradient takes hex values only, because a terminal color name is
    whatever the terminal's own scheme maps it to and there is no shade to interpolate. Every
    other token takes one color and says so if it is given either form.

    Every counted quantity owns two tokens, one for the number and one for the word beside it,
    so either can be picked out without touching the other. Each is named after the word that
    appears on screen:

      files-number       files-label
      lines-number       lines-label
      code-number        code-label
      comments-number    comments-label
      extra-number       extra-label
      total-size-number  total-size-label
      avg-size-number    avg-size-label
      keyword-number     keyword-label

    'size-unit' is the unit next to a size, the 'KB' of '430.5 KB total', and it is one token for
    both sizes since there is no reason to want two colors of KB on one line. It is separate from
    the labels so that it can stay quiet while 'Size' reads like every other column header.

    The numbers of the \"history\" section are the same quantities, so they follow the same tokens.

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

    The sections inside a container file, one token per column of the same row:
      nested-name              the name of a section language
      nested-branch            the tree characters that tie the sections to the row above
      nested-files             the file count of a section
      nested-lines             its lines
      nested-code              its code lines
      nested-comments          its comment lines
      nested-extra             its remaining lines
      nested-size              its size
      nested-size-unit         the unit beside that size
      nested-percent           the percentages of a section, which are of the container

    The rows of a '--by-file' run, which hang under a language beside those sections:
      file-name                the path of a file
      file-branch              the tree characters that tie the files to the language above
      file-files               the file count of such a row, which is always one
      file-lines               its lines
      file-code                its code lines
      file-comments            its comment lines
      file-extra               its remaining lines
      file-size                its size
      file-size-unit           the unit beside that size
      file-percent             the percentages of a file, which are of its language

    An '--explain' run, where the verdict words follow the label tokens above. The first two
    paint stretches inside the source lines, and what is neither keeps the terminal's own color:
      explain-string           the stretches of a line that sit inside a string
      explain-comment          the stretches that sit inside a comment
      explain-detail           the class name on a verdict row

    The overview:
      overview-label           the 'Files:', 'Lines:' and 'Size :' row labels
      overview-percent         the percentages of the overview
      bar-frame                the brackets around the overview bar and the live one
      language-1               the first language, its name and the color of its bar cells
      language-2               the second
      language-3               the third
      language-4               the fourth, shown only when nothing was folded into 'others'
      language-others          the folded 'others' entry. A theme that names 'language-4'
                               and not this one gets the same style here, since the two
                               never appear together

    A figure that moved, in the history section and in a '--diff' comparison alike:
      change-up                an increase
      change-down              a decrease
      change-same              no change

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

    The same tokens can be declared in a theme file and in the style block of a config. They are
    applied as one ladder of increasing specificity: the built-in defaults, then the theme, then
    this project's config, then '--style' for this run. So a theme can ship a complete look, your
    own config can keep a few tweaks that survive switching themes, and '--style' wins over both.

";
pub const THEME_EDITOR_HELP  :  &str =
"--theme-editor

    No arguments.

    Overrides normal program execution: generates an interactive HTML page with the language
    colors of every theme found in the persistent data path of the application, and opens it
    in the default browser. There, every color can be adjusted, with live contrast metrics and
    a mock overview drawn with the same bar character the program prints, and the result is
    turned into the five 'language-' lines of a theme file.

    Replaces the '--tune-palettes' command of previous versions.

";
pub const SHOW_THEMES_HELP  :  &str =
"--show-themes

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: medium

    Overrides normal program execution and just prints a sorted list with the names of all
    the themes that were detected in the persistent data path of the application, where you
    can add more, each one previewed on a sample of the real details rows and a mock overview.

    The preview follows '--layout', so it shows the shape a run would actually print.

    The optional argument draws the preview bar with the character that the '--bar-thickness'
    command would use, so that a theme can be judged the way it will be printed.

";
pub const LOG_HELP  :  &str =
"--log

    Can take 0..n words as arguments in the cmd.

    This flag only works if a configuration file is loaded. Specifies that a new log entry should be made
    with the stats of this program execution, inside the appropriate file in the 'data/logs' directory.
    If not log file exists for this configuration, one is created.
    All the provided arguments are used as a description of the log entry.

    A configuration file cannot declare it: logging is asked for per run, so that loading a
    configuration never writes an entry on its own. It does not apply to a '--diff' run either,
    since a comparison is not logged, and mezura says so instead of writing an entry.

";
pub const COMPARE_LEVEL_HELP  :  &str =
"--compare

    1 argument: a number between 0 and 10. Default: 1

    This flag only works if a configuration file is loaded. Specifies with how many previous logs this
    program execution should be compared to (see '--save' and '--load' commands).

    Providing 0 as argument will disable the history section (comparison).

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

    No arguments, or one number.

    Every counted file gets a row of its own, under the language it was counted as, showing its
    lines split into code, comments and everything else. Without a number every file is printed,
    the way '--top' shows every language until a number says otherwise.

    A number is how many to show under each language: '--by-file 20' prints the twenty biggest of
    every language, by whatever '--sort' is in effect, and 0 means all of them again. The cut is
    inside each language of each module, for the same reason '--top' cuts inside each module: over
    a whole report, the one part holding the biggest files would leave every other part with none.

    A language whose files were cut ends on a branch left hanging, so that a tree drawn shut is
    always the whole of what there is, and how many were left out is reported above the total. The
    files of a language '--top' hid are not printed, since their language is not there to sit under.

    A path too wide for its column loses whole directories out of its middle and never a piece of
    the file's own name. A file's keywords are not counted, so a row is one line whatever the
    language declares. The JSON document carries the same rows under each language, as 'by_file'.

";
pub const SAVE_HELP  :  &str =
"--save

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Doing so, will run the program and also create a .txt configuration file,
    inside 'data/config/' with the specified name, that can later be loaded with the --load command.

";
pub const LOAD_HELP  :  &str =
"--load

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Associated with the '--save' command, this command is used to load the flags of
    an existing configuration file from the 'data/config/' directory.

    You can combine the '--load' and '--save' commands to modify a configuration file.

";
pub const CHANGELOG_HELP  :  &str =
"--changelog

    No arguments, or the optional argument 'full'.

    Overrides normal program execution and just prints a summary of the changes
    of the current version of the program. If 'full' is provided, the changes
    of every previous version are printed too, most recent first.

";
pub const SHOW_LANGUAGES_HELP  :  &str =
"--show-languages

    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of
    all the supported languages that were detected in the persistent data path
    of the application, where you can add more.

";
pub const SHOW_CONFIGS_HELP  :  &str =
"--show-configs

    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of
    all the configuration files that were detected in the persistent data path
    of the application.

";

pub const VERSION_HELP  :  &str =
"--version

    No arguments.

    Overrides normal program execution and prints the version of this binary, with the date it
    was released on. An unreleased build says so instead of naming a date.

    Not to be confused with '--hide version', which only leaves the version line off the top of
    a normal run.

";
pub const HELP_HELP  :  &str =
"--help

    No arguments, or any number of other command names, written with their dashes:
    '--help --style --layout' explains those two and nothing else.

    Overrides normal program execution and prints this message.

";

// Grouped by what the commands are about and not by how they work, so a reader looking for themes
// finds '--show-themes' beside '--theme' and does not care that one of them overrides the run.
//
// The full help prints this, the lookup for one command searches it, and the close-match suggestions
// take their candidates from it, so a new command cannot reach one of the three and miss the others.
pub const COMMAND_HELP : [(&str, &[(&str, &str)]); 6] = [
    ("What is counted", &[
        (TARGETS, TARGETS_HELP),
        (EXCLUDE, EXCLUDE_HELP),
        (LANGUAGES, LANGUAGES_HELP),
        (EXCLUDE_LANGUAGES, EXCLUDE_LANGUAGES_HELP),
        (FORCE_LANGUAGE, FORCE_LANGUAGE_HELP),
        (NO_GITIGNORE, NO_GITIGNORE_HELP),
        (SEARCH_IN_DOTTED, SEARCH_IN_DOTTED_HELP),
        (SHOW_LANGUAGES, SHOW_LANGUAGES_HELP),
    ]),
    ("How the report looks", &[
        (COUNTING, COUNTING_HELP),
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

// The library gives its errors a plain 'Display'; this is the same text as this program says it,
// in its colors and broken to a width a person reads comfortably.
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

// Broken between words and never inside one, and a line that was already short is left alone. Lines
// the message wrote itself are kept, so a message that laid itself out is not laid out twice.
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

// Used both by the full help and by the test that writes the README's command list, so that the two
// cannot describe the same commands differently or in a different order
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

// The date lives in the Changelog's first line, 'v3.0.0 - unreleased', and nowhere else: a constant
// would be a third place to remember on every release and the one most likely to be forgotten. A
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

pub fn print_whole_help_message() {

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
    msg += &create_help_body();

    println!("{msg}");
}

pub fn print_help_message_for_given_args(args_line: &str) {
    let options = crate::args::split_into_command_segments(args_line).into_iter().skip(1).collect::<Vec<_>>();
    if options.len() == 1 {
        print_whole_help_message();
        return;
    }

    // The first '--help' is the command being run and not something it was asked about, so it is
    // skipped once. A second one is a real question, and a third says nothing the second did not,
    // so a name is answered once however many times it was typed.
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
            Some(x) => entries += x,
            // The same error the program gives without '--help', so that an unknown command does
            // not read as an ordinary line of help text
            None => entries += &format!("{}\n\n", ArgParsingError::UnrecognisedCommand(name.to_owned()).format())
        }
    }

    // The data dir line is always there, so asking whether the message is empty never answered
    // anything: nothing recognised has to be counted on its own
    if entries.is_empty() {
        print_whole_help_message();
    } else {
        println!("{}{entries}", get_data_dir_str());
    }
}

// On the error output, where all 29 of its callers already are: every one is a path that returns a
// failure, and on stdout this text was what a redirected '--output json > stats.json' ended up
// holding instead of a document.
pub fn print_help_message_for_command(arg: &str) {
    if let Some(x) = get_help_msg_of_command(arg) {
        eprintln!("\n{x}");
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

// A theme is 46 tokens, and the one place whose job is to show what one looks like before you pick
// it has to show more than the four language slots of a mock overview line. The sample includes a
// real details row, which is the densest line the program prints.
pub fn print_existing_themes(bar_thickness: BarThickness, layout: Layout, counting: CountingModel) {
    // Five entries, so that every language slot including the fold gets to show itself. The
    // verticals add up to the width of a real bar.
    const MOCK_PERCENTAGES : [(&str, f64, usize); 5] =
            [("first", 40.0, 20), ("second", 26.0, 13), ("third", 16.0, 8), ("fourth", 10.0, 5), ("others", 8.0, 4)];
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
        // One blank line more between two themes than any block leaves inside one, so that a long
        // listing reads as a stack of samples rather than one run-on page
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
            msg.push_str(&format!("{} {}", theme.overview_percent.paint(&format!("{percentage:>5.2}%")), slots[i].paint(lang)));
            if i < MOCK_PERCENTAGES.len()-1 {msg.push_str(" - ")}
        }

        // On its own line, under the percentages: five language slots plus a fifty cell bar do not
        // fit next to each other, and this listing has no reason to be the widest thing printed
        msg.push_str(&format!("\n{INDENT}{}{}", " ".repeat(BAR_INDENT), theme.bar_frame.paint("[-")));
        for (i, (_, _, verticals)) in MOCK_PERCENTAGES.iter().enumerate() {
            let cell = bar_thickness.get_character().repeat(*verticals);
            msg.push_str(&match slots[i].get_color() {
                Some(color) => cell.color(color).to_string(),
                None => cell
            });
        }
        msg.push_str(&format!("{}\n", theme.bar_frame.paint("-]")));
    }

    println!("{msg}");
}

pub fn print_supported_languages(languages_available: &[Language]) {
    println!("{}", format_supported_languages_message(languages_available));
}

pub fn print_existing_configs() {
    let mut config_names = Vec::with_capacity(10);

    let Ok(config_dir) = fs::read_dir(&PERSISTENT_APP_PATHS.config_dir) else {
        println!("{}","Could not read the config dir".yellow());
        return;
    };
    for path in config_dir.flatten() {
        if let Ok(f) = path.file_type() && f.is_file() {
            config_names.push(path.file_name())
        }
    }
    let mut config_names = config_names.iter().filter_map(|x| {
        // Skipped rather than shown lossily: this list exists to be typed back into '--load'
        let str = x.to_str()?;
        if str != "default.txt" {
            Some(str)
        } else {
            None
        }
    }).collect::<Vec<_>>();
    config_names.sort_unstable();
    println!("{}", format_existing_configs_message(&config_names));
}

// The reason travels beside each name, which is the shape the faulty counted files already use. One
// heading over both reasons a file can fail is true of only one of them: a file saved in an encoding
// that cannot be read as text would be announced as a file with a typo in it, and its owner would go
// hunting for a mistake that is not there.
pub fn format_faulty_language_files_message(faulty_files: &[FaultyLanguageFile]) -> String {
    let mut message = format!("\n{} language {} could not be used, and will not be taken into consideration.",
            faulty_files.len(), if faulty_files.len() == 1 {"file"} else {"files"});
    for faulty in faulty_files {
        message += &format!("\n-- {}: {}", faulty.file_name, faulty.error);
    }
    message + "\n"
}

// Split from the printing so that the deduplication can be asserted. Two files declaring one
// language is a broken installation, and naming it twice here would read as two languages rather
// than as the one it is.
fn format_supported_languages_message(languages_available: &[Language]) -> String {
    const COLUMNS : usize = 3;

    let mut lang_names = languages_available.iter().map(|x| x.name.to_owned()).collect::<Vec<_>>();
    lang_names.sort();
    lang_names.dedup();
    format!("{}The supported languages found are:\n\n{}\n", get_data_dir_str(),
            format_in_columns(&lang_names, COLUMNS))
}

// Filled downwards and not across, so that a sorted list still reads in order down each column
// instead of jumping from one to the next and back on every name.
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

// Split from the printing so that the empty case can be asserted: joining the names with the same
// two spaces that indent the first one leaves a heading over a line holding those two spaces and
// nothing else, which reads as a configuration whose name failed to appear rather than as none
// existing. The data dir is named either way, being the answer to "where would I put one".
fn format_existing_configs_message(config_names: &[&str]) -> String {
    if config_names.is_empty() {
        format!("{}No configurations found.\n", get_data_dir_str())
    } else {
        format!("{}Found these configurations:\n  {}\n", get_data_dir_str(), config_names.join("\n  "))
    }
}

fn get_data_dir_str() -> String {
    format!("\nData dir path: {}\n\n", PERSISTENT_APP_PATHS.data_dir)
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
        // the words survive whole and in order, and the two sentences still start on their own lines
        assert_eq!(long.split_whitespace().collect::<Vec<_>>(), wrapped.split_whitespace().collect::<Vec<_>>());
        assert!(wrapped.lines().any(|x| x.starts_with("Use the '--no-gitignore'")));

        // a word longer than the width has nowhere to break, so it goes out whole on its own line
        let path = "a/".repeat(MESSAGE_WIDTH);
        assert_eq!(format!("see\n{path}"), wrap_message(&format!("see {path}")));
    }

    // An installation holding two files that declare one language is one language however many
    // files describe it. The list used to arrive as a map, which deduplicated it without anybody
    // deciding to, and the line that took that job over had nothing asserting it.
    #[test]
    fn a_language_declared_by_two_files_is_listed_once() {
        let twice = vec![Language::new("Java", ["java"], ["\""], ["//"], &[], []),
                Language::new("Rust", ["rs"], ["\""], ["//"], &[], []),
                Language::new("Java", ["jav"], ["\""], ["//"], &[], [])];
        let listed = format_supported_languages_message(&twice);

        assert_eq!(1, listed.matches("Java").count(), "'Java' was listed more than once:\n{listed}");
        assert!(listed.contains("Rust"));
    }

    // The columns are filled downwards, so the last one is short by however much the count misses a
    // multiple of three, and that ragged end is where a name goes missing without anything else
    // looking wrong.
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
    // no ordinary run prints it and no comparison of two builds can see it. Both reasons a file can
    // fail used to arrive under one heading calling them formatting problems, which sent the owner of
    // a file saved in the wrong encoding looking for a typo that was not there.
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
        // each named on its own line, with the reason that belongs to it and not to the other, and
        // the malformed one names the line, since the blocks have to arrive in one fixed order
        assert!(both.contains("-- Utf16.txt: the language file could not be read"), "{both}");
        assert!(both.contains("-- Garbage.txt: line 7 is not what the format expects there"), "{both}");
        assert!(!both.contains("Formatting problems"), "the two reasons are under one wrong heading again:\n{both}");

        let one = format_faulty_language_files_message(&[FaultyLanguageFile {
            file_name: "Garbage.txt".to_owned(),
            error: mezura_core::language_file::LanguageFileError::Malformed(1)}]);
        assert!(one.contains("1 language file could not be used"), "{one}");
    }

    // With nothing to list, '--show-configs' printed the heading and then a line holding the two
    // spaces that indent a name, which reads as a name that failed to print rather than as an
    // empty data dir.
    #[test]
    fn an_empty_config_dir_says_so_instead_of_listing_nothing() {
        let none = format_existing_configs_message(&[]);
        assert!(none.contains("No configurations found."), "{none}");
        assert!(!none.contains("Found these configurations"), "{none}");
        assert!(!none.contains("\n  \n"), "an empty bullet was printed:\n{none}");

        let some = format_existing_configs_message(&["mezura.txt", "portal.txt"]);
        assert!(some.contains("Found these configurations:\n  mezura.txt\n  portal.txt"), "{some}");
        // and either way the reader is told where the directory is
        assert!(none.contains("Data dir path:") && some.contains("Data dir path:"));
    }

    const README_HEADING : &str = "## Cmd Commands";
    const FENCE : &str = "```";

    // Returns the command block of the README, and everything before and after it, so that the block
    // can be replaced without the rest of a hand written document being touched
    fn readme_parts(readme: &str) -> (String, String, String) {
        let heading_at = readme.find(README_HEADING).expect("the README has a '## Cmd Commands' heading");
        let opening = readme[heading_at..].find(FENCE).expect("that section opens a fenced block") + heading_at;
        let body_at = opening + FENCE.len();
        let closing = readme[body_at..].find(FENCE).expect("that fenced block is closed") + body_at;

        (readme[..body_at].to_owned(), readme[body_at..closing].to_owned(), readme[closing..].to_owned())
    }

    // The README's command list is not maintained, it is written from the help texts, which are the
    // one source. Everything the README wants to say that the help does not say has to live outside
    // the fence, because the inside of it is replaced wholesale.
    #[test]
    fn the_readme_command_list_is_the_help_itself() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("README.md");
        let readme = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let (before, block, after) = readme_parts(&readme);
        let generated = create_help_body();

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, format!("{before}\n{}\n{after}", generated.trim_end())).unwrap();
            return;
        }

        assert_eq!(block.trim(), generated.trim(),
                "the command list in the README no longer matches the help texts, which are the source. \
                 Regenerate it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme");
    }

    // A token nobody can find is a token nobody can use, and the help is the only place that lists
    // them. The README carries the same list by being generated from this text.
    #[test]
    fn every_style_token_is_named_in_the_help() {
        for token in crate::theme::Theme::get_token_names() {
            assert!(STYLE_HELP.contains(token), "'{token}' is a style token that the help never names");
        }
    }

    // '--version' reads the release date from the first line of the Changelog, so that line has to
    // keep naming the version this binary reports. Without this the two drift apart silently and
    // '--version' starts quoting the date of a release that is not the one running.
    #[test]
    fn the_changelog_opens_with_the_version_this_binary_reports() {
        let changelog = String::from_utf8_lossy(CHANGELOG_BYTES);
        let first = changelog.lines().next().unwrap();
        assert!(first.starts_with(&format!("{VERSION_ID} - ")),
                "the Changelog opens with '{first}', which does not start with '{VERSION_ID} - '");
    }

    // The three lists of commands became one table, and nothing else may hold a fourth
    #[test]
    fn every_command_has_exactly_one_help_entry() {
        let names = get_command_names();
        for name in &names {
            assert!(get_help_msg_of_command(name).is_some(), "'--{name}' has no help entry");
            assert_eq!(1, names.iter().filter(|x| *x == name).count(), "'--{name}' is listed twice");
        }
    }
}
