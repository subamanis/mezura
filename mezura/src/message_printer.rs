use std::{collections::HashMap, fs};

use colored::Colorize;

use super::formatted::Formatted;
use crate::paths::PERSISTENT_APP_PATHS;
use mezura_core::Language;

// The file itself, so that the command never depends on an installation having a copy of it.
static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");
use super::config_manager::*;

// These constants need to be maintained along with the readme's commands
pub const DIRS_HELP  :  &str =
"--dirs

    The paths to the directories or files, separated by commas if more than 1,
    in this form: '--dirs <path1>, <path2>'
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

    The given language names must exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    Only the languages specified here will be taken into account for the stats.

";
pub const EXCLUDE_LANGUAGES_HELP  :  &str =
"--exclude-languages

    1..n arguments separated by commas, case-insensitive

    The given language names should exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    The given language names will be ignored from the stats calculation, if they exist.

";
pub const FORCE_LANG_HELP  :  &str =
"--force-lang

    1..n pairs of 'extension=language' separated by commas, case-insensitive

    Decides which language an extension is counted as, whether or not another language claims it:
    '--force-lang m=matlab,pl=perl,txt=python'

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
pub const BRACES_AS_CODE_HELP  :  &str =
"--braces-as-code

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable, or 'no'
    to disable. Default: no

    Specifies whether lines that carry no content should be considered as code lines or not.
    A line carries no content when nothing but punctuation is left on it once the strings and
    the comments are taken out: '}', '});', '],', ')'.

    The default behaviour is to not count them as code, since it is silly for code of the same content
    and substance to be counted differently, according to the programmer's code style.
    Writing 'if (x) { do(); }' on one line or on three should not change the number.
    This helps to keep the stats clean when using code lines as a complexity and productivity metric.

    Note that other line counters count these lines as code, so this is where their number and
    mezura's differ. Enabling this flag makes the two closer.

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

";
pub const HIDE_HELP  :  &str =
"--hide

    One or more names separated by commas or spaces, for example:
    --hide parsing-info,timing   or   --hide parsing-info timing

    Leaves the named parts of the output unprinted. What you can hide:

      version         the version line at the top
      directory-info  the 'Analyzing directories' line and the 'N files found' line under it
      parsing-info    the 'Parsing files' line and the 'ok' under it
      keywords        the keyword counts, keeping the rest of the details rows
      overview        the whole percentages section
      bar             only the [-|||-] bar of the overview, keeping the percentages and the colors
      progress        the comparison with previous runs (the same as '--compare 0')
      timing          the execution time line at the bottom

    The list mixes whole sections with parts of them on purpose: you are pointing at what you
    see, not at how the program is organised.

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

    One argument: 'lines', 'files', 'code', 'size' or 'name'. Default: lines

    Chooses the order of the languages in the \"details\" section, which also decides which of them
    reach the \"overview\" section and which are folded into its 'others' entry.
    Every criterion except 'name' sorts from the largest down, and ties are broken alphabetically
    so that the order never changes between runs on the same data.

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

    This already happens on its own when you install a new version, so you should not need it. It
    is here for when something was damaged or deleted within one version, where nothing else would
    notice.

    A language file you changed is replaced too, since one that has fallen behind counts wrongly,
    but your copy is kept under 'data/replaced/<version>/' and named, so you can carry your changes
    over. A language file of your own is never touched, and neither are your themes, your default
    configuration or 'extension_priority.txt': those are written when absent and left alone after.

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

    'size-unit' is the unit next to a size, the 'KBs' of '430.5 KBs total', and it is one token for
    both sizes since there is no reason to want two colors of KBs on one line. It is separate from
    the labels so that it can stay quiet while 'Size' reads like every other column header.
      keyword-number     keyword-label

    The numbers of the \"progress\" section are the same quantities, so they follow the same tokens.

    The rest:
      version            the version line at the top
      heading            the section titles and the 'Analyzing directories' lines
      separator          the dashed line above the total
      arrow              the '->' after a language name, in the 'list' layout only
      bar-frame          the '[-' and '-]' around the overview bar
      percent            the percentages of the details rows
      details-language-header  the word 'Language' over the first column of the two tables
      details-language-name    the name of a language, in a row and in the keywords block
      details-total      the word 'Total'
      overview-label     the 'Files:', 'Lines:' and 'Size :' row labels of the overview
      overview-percent   the percentages of the overview
      language-1         the first language of the overview, its name and the color of its bar cells
      language-2         the second
      language-3         the third
      language-4         the fourth, shown only when nothing was folded into 'others'
      language-others    the folded 'others' entry. A theme that names 'language-4' and not this one
                         gets the same style here, since the two never appear together
      progress-up        an increase in the progress section
      progress-down      a decrease
      progress-same      no change
      progress-entry     the '->' of a progress entry
      progress-modified  the word 'modified:' on an entry that was counted with other settings
      progress-modified-field  the names of the settings that changed since that entry
      summary            the found / of interest / excluded line
      note               the '(+N more languages hidden by --top N)' line
      success            the 'ok' after parsing
      warning            warnings
      error              errors
      footer             the execution time line

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
    If specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    This flag only works if a configuration file is loaded. Specifies that a new log entry should be made
    with the stats of this program execution, inside the appropriate file in the 'data/logs' directory.
    If not log file exists for this configuration, one is created.
    All the provided arguments are used as a description of the log entry.

    This flag will not be saved in a configuration file automatically, but it can be added manually.

";
pub const COMPRARE_LEVEL_HELP  :  &str =
"--compare

    1 argument: a number between 0 and 10. Default: 1

    This flag only works if a configuration file is loaded. Specifies with how many previous logs this
    program execution should be compared to (see '--save' and '--load' commands).

    Providing 0 as argument will disable the progress report (comparison).

    Every log entry records the settings that decide what is counted, and an entry that was written
    with different ones is marked 'modified:' followed by their names. The comparison is still shown,
    because the point is to say whether it can be trusted: a change of 'dirs' means the numbers came
    from another tree, while a change of 'braces-as-code' means lines moved between code and extra
    and the total did not. An entry written by a version that did not record a setting is never
    reported as having changed it.

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

// The one list of commands, grouped by what they are about and not by how they work, which is why
// the ones that print instead of counting are spread across the groups they belong to rather than
// sitting together: a reader looking for themes wants '--show-themes' next to '--theme', and does
// not care that one of them overrides the run. The full help prints it in this order, the lookup for
// a single command searches it, and the close-match suggestions take their candidates from it, so a
// new command cannot appear in one of the three and be forgotten in the others.
pub const COMMAND_HELP : [(&str, &[(&str, &str)]); 6] = [
    ("What is counted", &[
        (DIRS, DIRS_HELP),
        (EXCLUDE, EXCLUDE_HELP),
        (LANGUAGES, LANGUAGES_HELP),
        (EXCLUDE_LANGUAGES, EXCLUDE_LANGUAGES_HELP),
        (FORCE_LANG, FORCE_LANG_HELP),
        (NO_GITIGNORE, NO_GITIGNORE_HELP),
        (SEARCH_IN_DOTTED, SEARCH_IN_DOTTED_HELP),
        (BRACES_AS_CODE, BRACES_AS_CODE_HELP),
        (SHOW_LANGUAGES, SHOW_LANGUAGES_HELP),
    ]),
    ("How the report looks", &[
        (LAYOUT, LAYOUT_HELP),
        (SORT, SORT_HELP),
        (TOP, TOP_HELP),
        (HIDE, HIDE_HELP),
        (THEME, THEME_HELP),
        (STYLE, STYLE_HELP),
        (BAR_THICKNESS, BAR_THICKNESS_HELP),
        (NUMBER_SEPARATOR, NUMBER_SEPARATOR_HELP),
        (DECIMAL_SEPARATOR, DECIMAL_SEPARATOR_HELP),
        (SHOW_THEMES, SHOW_THEMES_HELP),
        (THEME_EDITOR, THEME_EDITOR_HELP),
    ]),
    ("Taking the result elsewhere", &[
        (OUTPUT, OUTPUT_HELP),
        (LOG, LOG_HELP),
        (COMPRARE_LEVEL, COMPRARE_LEVEL_HELP),
    ]),
    ("Your data directory", &[
        (SAVE, SAVE_HELP),
        (LOAD, LOAD_HELP),
        (SAVE_THEME, SAVE_THEME_HELP),
        (SHOW_CONFIGS, SHOW_CONFIGS_HELP),
        (RESTORE, RESTORE_HELP),
    ]),
    ("Tuning and diagnostics", &[
        (THREADS, THREADS_HELP),
        (SHOW_FAULTY_FILES, SHOW_FAULTY_FILES_HELP),
    ]),
    ("The program itself", &[
        (HELP, HELP_HELP),
        (VERSION, VERSION_HELP),
        (CHANGELOG, CHANGELOG_HELP),
    ]),
];

// Used both by the full help and by the test that writes the README's command list, so that the two
// cannot describe the same commands differently or in a different order
pub fn help_body() -> String {
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

// The date lives in the first line of the Changelog, 'v3.0.0 - unreleased' or 'v2.0.1 - 27/7/2026',
// and nowhere else. A separate constant would be a third place to remember on every release, next to
// VERSION_ID and Cargo.toml, and the one most likely to be forgotten. The test in this module is
// what keeps that first line and VERSION_ID from drifting apart.
pub fn print_version() {
    let changelog = String::from_utf8_lossy(CHANGELOG_BYTES);
    let released = changelog.lines().next().unwrap_or_default().split_once(" - ")
            .map_or_else(|| "unreleased".to_owned(), |(_, date)| date.trim().to_owned());

    println!("
{} ({released})
", super::theme::active().version.paint(VERSION_ID));
}

pub fn command_names() -> Vec<&'static str> {
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
    msg += &help_body();

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
            None => entries += &format!("{}\n\n", ArgParsingError::UnrecognisedCommand(name.to_owned()).formatted())
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

pub fn print_help_message_for_command(arg: &str) {
    if let Some(x) = get_help_msg_of_command(arg) {
        println!("\n{x}");
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

// A theme is 46 tokens and the listing used to show five of them, the language slots, on a mock
// overview line. So the one place whose job is to show what a theme looks like before you pick it
// said nothing about its headings, labels, numbers, percentages, arrow, separator or size figures.
// It now prints a sample of the real details rows too, which is the densest line the program has.
pub fn print_existing_themes(bar_thickness: BarThickness, layout: Layout) {
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
    for name in theme_names.iter() {
        msg.push_str(&format!("\n  {}\n\n", name.bold()));

        let Some((styles, _)) = super::theme_files::load_theme(name, &PERSISTENT_APP_PATHS.themes_dir) else {
            msg.push_str(&format!("{INDENT}{}\n","(this theme could not be read)".yellow()));
            continue;
        };
        let theme = super::theme::resolve(&styles, &[], &[]);

        msg.push_str(&format!("{INDENT}{}.\n", theme.heading.paint("Details")));
        for row in super::result_printer::theme_sample_rows(&theme, layout) {
            msg.push_str(&format!("{INDENT}{row}\n"));
        }

        msg.push_str(&format!("\n{INDENT}{}.\n", theme.heading.paint("Overview")));
        msg.push_str(&format!("{INDENT}{}   ", theme.overview_label.paint("Lines:")));
        let slots = theme.language_slots();
        for (i, (lang, percentage, _)) in MOCK_PERCENTAGES.iter().enumerate() {
            msg.push_str(&format!("{} {}", theme.overview_percent.paint(&format!("{percentage:>5.2}%")), slots[i].paint(lang)));
            if i < MOCK_PERCENTAGES.len()-1 {msg.push_str(" - ")}
        }

        // On its own line, under the percentages: five language slots plus a fifty cell bar do not
        // fit next to each other, and this listing has no reason to be the widest thing printed
        msg.push_str(&format!("\n{INDENT}{}{}", " ".repeat(BAR_INDENT), theme.bar_frame.paint("[-")));
        for (i, (_, _, verticals)) in MOCK_PERCENTAGES.iter().enumerate() {
            let cell = bar_thickness.character().repeat(*verticals);
            msg.push_str(&match slots[i].color {
                Some(color) => cell.color(color).to_string(),
                None => cell
            });
        }
        msg.push_str(&format!("{}\n", theme.bar_frame.paint("-]")));
    }

    println!("{msg}");
}

pub fn print_supported_languages(languages_map: &HashMap<String,Language>) {
    let mut lang_names = languages_map.keys().map(|x| x.to_owned()).collect::<Vec<_>>();
    lang_names.sort();
    let prefix = get_data_dir_str();
    println!("{}The supported languages found are:\n  {}\n",prefix,lang_names.join("\n  "));
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
    let prefix = get_data_dir_str();
    println!("{}Found these configurations:\n  {}\n",prefix,config_names.join("\n  "));
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
        let generated = help_body();

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, format!("{before}\n{}\n{after}", generated.trim_end())).unwrap();
            return;
        }

        assert_eq!(block.trim(), generated.trim(),
                "the command list in the README no longer matches the help texts, which are the source. \
                 Regenerate it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura readme");
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
        let names = command_names();
        for name in &names {
            assert!(get_help_msg_of_command(name).is_some(), "'--{name}' has no help entry");
            assert_eq!(1, names.iter().filter(|x| *x == name).count(), "'--{name}' is listed twice");
        }
    }
}
