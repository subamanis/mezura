use std::{collections::HashMap, fs};

use colored::Colorize;

use crate::{CHANGELOG_BYTES, Formatted, Language, PERSISTENT_APP_PATHS, config_manager::*, io_handler};

// These constants need to be maintained along with the readme's commands
pub const DIRS_HELP  :  &str =
"--dirs

    The paths to the directories or files, separated by commas if more than 1,
    in this form: '--dirs <path1>, <path2>'
    A path can also be a glob pattern (* ? [..] {..}), which is expanded to every existing
    directory and file that it matches, so 'services/*/src' is a valid target.
    Since the matches of a pattern are found by the program and not named by you, they follow
    the same rules as every other path it discovers: the ones that a .gitignore ignores, or that
    are dotted, are skipped (see the '--no-gitignore' and '--search-in-dotted' commands).
    A path that you write out explicitly is always used, even if it is ignored or dotted.
    Targets that are contained in other targets are dropped, so that no file is counted twice.
    If you are using Windows Powershell, you will need to escape the commas with a backtick: `
    or surround all the arguments with quotation marks:
    <path1>`, <path2>`, <path3>   or   \"<path1>, <path2>, <path3>\"

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
pub const THREADS_HELP  :  &str =
"--threads

    2 numbers: the first between 1 and 8 and the second between 1 and 30.

    This represents the number of the producers (threads that will traverse the given directories),
    and consumers (threads that will parse whatever files the producers found).

    If this command is not provided, the numbers will be chosen based on the available threads
    on your machine. Generally, a good ratio of producers-consumers is 1:3

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
    like .vscode or .git.

";
pub const SHOW_FAULTY_FILES_HELP  :  &str =
"--show-faulty-files

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no

    Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
    or while reading its contents. The default behavior when this happens is to count all of
    the faulty files and display their count.

    This flag specifies that their path, along with information about the exact error is displayed too.
    The most common reason for this error is if a file contains non UTF-8 characters.

";
pub const HIDE_HELP  :  &str =
"--hide

    One or more names separated by commas or spaces, for example:
    --hide status,timing   or   --hide status timing

    Leaves the named parts of the output unprinted. What you can hide:

      version     the version line at the top
      status      the 'Analyzing directories', 'Parsing files' and 'N files found' lines
      details     the per language rows and the total underneath them
      keywords    the keyword counts, keeping the rest of the details rows
      overview    the whole percentages section
      bar         only the [-|||-] bar of the overview, keeping the percentages and the colors
      progress    the comparison with previous runs (the same as '--compare 0')
      timing      the execution time line at the bottom

    The list mixes whole sections with parts of them on purpose: you are pointing at what you
    see, not at how the program is organised.

    Errors and warnings are never hidden. Hiding the status still reports files that failed to
    be parsed, since otherwise the numbers would silently be wrong.

    Replaces the '--no-visual' and '--no-keywords' commands of previous versions.

";
pub const COLORS_HELP  :  &str =
"--colors

    1 to 5 colors separated by spaces. A color is either a hex value, with or without a leading
    '#' (e.g. ff8800 #00ff00), or one of the 16 standard terminal color names (black, red, green,
    yellow, blue, magenta, cyan, white and their bright- variants, e.g. bright-magenta).

    Overrides the colors used in the \"overview\" section of the results: the first three colors
    are used for the three most relevant languages, the fourth for a fourth language, and the
    optional fifth for the 'others' entry (if omitted, the fourth is used for 'others' too).
    If fewer colors are provided, the remaining ones keep their default color.

    Named colors follow the terminal's color scheme; hex colors are rendered with 24-bit ANSI
    codes, so the terminal must support truecolor.
    If you are using Windows Powershell, either omit the '#' or surround the color with
    quotation marks (\"#ff8800\"), since an unquoted '#' starts a comment.

";
pub const COLOR_PALETTE_HELP  :  &str =
"--color-palette

    One argument, the name of a palette (case-insensitive).

    Applies a named color palette. Palettes are .txt files in the 'data/palettes' dir, in the
    persistent data path of the application, where the file name is the palette name. Each line
    is a 'token = value' pair: 'languages' takes 1 to 5 colors in the same format as the
    '--colors' command, and any style token (see '--help style') takes a style.
    You can add your own palettes there.
    Use the '--show-palettes' command to list the available palettes.

    If the '--colors' command is also provided, it takes precedence over the palette's
    'languages' line, while the palette's style tokens still apply.
    Palettes written for versions before v3.0.0 are converted automatically on the first run.

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

";
pub const BAR_THICKNESS_HELP  :  &str =
"--bar-thickness

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: slim

    Chooses the character that the percentage bar of the \"overview\" section is drawn with.

      slim     |   the default, and the only one made of ASCII, so it is the one that is
                   guaranteed to render on every terminal
      medium   ┃   thicker, but still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a terminal and a font that can render box drawing characters.
    If the bar comes out as question marks or empty boxes, use 'slim'.

";
pub const STYLE_HELP  :  &str =
"--style

    One or more 'token=style' pairs separated by commas, for example:
    --style number=bright-black,label=b5a98a italic,heading=white bold underline

    Overrides how a category of printed text looks. A style is a color and any number of the
    attributes 'bold', 'italic', 'underline', 'dim' and 'reverse', in any order. The color is
    either a hex value or one of the 16 terminal color names (the same grammar as '--colors'),
    or the word 'default' to leave the terminal's own foreground color alone.

    'reverse' swaps the text and background colors, so it stands out strongly without committing
    to any color of its own, which means it adapts to whatever theme the terminal is using.

    The tokens are:
      heading          the section titles and the 'Analyzing directories' lines
      separator        the dashed line above the total
      bar-frame        the '[-' and '-]' around the overview bar
      number           every counted value
      percent          every percentage
      label            the words 'files', 'lines', 'KBs total' and so on
      overview-label   the 'Files:', 'Lines:' and 'Size :' row labels of the overview
      details-language  a language name in the details rows
      overview-language the language names in the overview. A color declared here is ignored, since
                        the overview colors each language from the palette, but the attributes apply
      details-total     the word 'Total'
      keyword          the names of the counted keywords
      progress-up      an increase in the progress section
      progress-down    a decrease
      progress-same    no change
      progress-entry   the '->' of a progress entry
      summary          the found / of interest / excluded line
      success          the 'ok' after parsing
      warning          warnings
      error            errors
      footer           the execution time line

    Styles can also be declared inside a color palette or in a config file. They are applied in
    that order, so a palette can ship a complete look while your own config keeps your
    preferences across palette changes, and '--style' wins over both.

";
pub const TUNE_PALETTES_HELP  :  &str =
"--tune-palettes

    No arguments.

    Overrides normal program execution: generates an interactive HTML page with all the color
    palettes found in the persistent data path of the application, and opens it in the default
    browser. There, every color of every palette can be adjusted, with live contrast metrics
    and a mock overview, and the result is turned into a ready '--colors' command that you can
    use directly or save in a palette file.

";
pub const SHOW_PALETTES_HELP  :  &str =
"--show-palettes

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: slim

    Overrides normal program execution and just prints a sorted list with the names of
    all the color palettes that were detected in the persistent data path
    of the application, where you can add more, each one previewed on a mock overview.

    The optional argument draws the preview bars with the character that the
    '--bar-thickness' command would use, so that a palette can be judged the way it will
    actually be printed.

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
    msg += "Format of arguments: <path_here> --optional_command1 --optional_commandN\n\nCOMMANDS:\n\n";

    msg += CHANGELOG_HELP;
    msg += SHOW_LANGUAGES_HELP;
    msg += SHOW_CONFIGS_HELP;
    msg += SHOW_PALETTES_HELP;
    msg += TUNE_PALETTES_HELP;
    msg += DIRS_HELP;
    msg += EXCLUDE_HELP;
    msg += LANGUAGES_HELP;
    msg += EXCLUDE_LANGUAGES_HELP;
    msg += THREADS_HELP;
    msg += BRACES_AS_CODE_HELP;
    msg += SEARCH_IN_DOTTED_HELP;
    msg += SHOW_FAULTY_FILES_HELP;
    msg += NO_GITIGNORE_HELP;
    msg += HIDE_HELP;
    msg += COLORS_HELP;
    msg += COLOR_PALETTE_HELP;
    msg += SORT_HELP;
    msg += TOP_HELP;
    msg += BAR_THICKNESS_HELP;
    msg += STYLE_HELP;
    msg += LOG_HELP;
    msg += COMPRARE_LEVEL_HELP;
    msg += SAVE_HELP;
    msg += LOAD_HELP;

    println!("{msg}");
}

pub fn print_help_message_for_given_args(args_line: &str) {
    let options = args_line.split("--").skip(1).collect::<Vec<_>>();
    if options.len() == 1 {
        print_whole_help_message();
        return;
    }

    let mut msg = get_data_dir_str();

    for option in options {
        if option.trim().is_empty() {continue;}
        let sliced = option.split_whitespace().collect::<Vec<_>>();

        if let Some(x) = get_help_msg_of_command(sliced[0]) {
            msg += x;
        } else if sliced[0].trim() != HELP {
            // The same error the program gives without '--help', so that an unknown command
            // does not read as an ordinary line of help text
            msg += &format!("{}\n\n", ArgParsingError::UnrecognisedCommand(sliced[0].to_owned()).formatted());
        }
    }

    if msg.is_empty() {
        print_whole_help_message();
    } else {
        println!("{msg}");
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

pub fn print_existing_palettes(bar_thickness: BarThickness) {
    // The percentages of a mock "overview" line, used to preview every palette
    const MOCK_PERCENTAGES : [(&str, f64, usize); 4] =
            [("first", 40.0, 20), ("second", 30.0, 15), ("third", 20.0, 10), ("fourth", 10.0, 5)];

    let mut palette_names = Vec::with_capacity(10);
    let Ok(palettes_dir) = fs::read_dir(&PERSISTENT_APP_PATHS.palettes_dir) else {
        println!("{}","Could not read the palettes dir".yellow());
        return;
    };
    for path in palettes_dir.flatten() {
        if let Ok(f) = path.file_type() && f.is_file()
            && let Some(stem) = path.path().file_stem().and_then(|x| x.to_str()) {
            palette_names.push(stem.to_owned());
        }
    }
    palette_names.sort_by_key(|x| x.to_lowercase());

    let mut msg = get_data_dir_str();
    msg.push_str("Found these color palettes:\n");
    for name in palette_names.iter() {
        msg.push_str(&format!("\n  {}\n     ", name.bold()));

        let Some(colors) = io_handler::load_palette(name, &PERSISTENT_APP_PATHS.palettes_dir).and_then(|x| x.languages) else {
            msg.push_str(&format!("{}\n","(the colors of this palette could not be parsed)".yellow()));
            continue;
        };

        for (i, (lang, percentage, _)) in MOCK_PERCENTAGES.iter().enumerate() {
            let color = colors[i.min(colors.len()-1)];
            msg.push_str(&format!("{:>5.2}% {}", percentage, lang.color(color)));
            if i < MOCK_PERCENTAGES.len()-1 {msg.push_str(" - ")}
        }

        msg.push_str("    [-");
        for (i, (_, _, verticals)) in MOCK_PERCENTAGES.iter().enumerate() {
            let color = colors[i.min(colors.len()-1)];
            msg.push_str(&bar_thickness.character().repeat(*verticals).color(color).to_string());
        }
        msg.push_str("-]\n");
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
        let str = x.to_str().unwrap();
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

fn get_help_msg_of_command(command: &str) -> Option<&str> {
    if command == DIRS {
        Some(DIRS_HELP)
    } else if command == EXCLUDE {
        Some(EXCLUDE_HELP)
    } else if command == LANGUAGES {
        Some(LANGUAGES_HELP)
    } else if command == EXCLUDE_LANGUAGES {
        Some(EXCLUDE_LANGUAGES_HELP)
    } else if command == THREADS {
        Some(THREADS_HELP)
    } else if command == BRACES_AS_CODE {
        Some(BRACES_AS_CODE_HELP)
    } else if command == SEARCH_IN_DOTTED {
        Some(SEARCH_IN_DOTTED_HELP)
    } else if command == SHOW_FAULTY_FILES {
        Some(SHOW_FAULTY_FILES_HELP)
    } else if command == HIDE {
        Some(HIDE_HELP)
    } else if command == NO_GITIGNORE {
        Some(NO_GITIGNORE_HELP)
    } else if command == COLORS {
        Some(COLORS_HELP)
    } else if command == COLOR_PALETTE {
        Some(COLOR_PALETTE_HELP)
    } else if command == SORT {
        Some(SORT_HELP)
    } else if command == TOP {
        Some(TOP_HELP)
    } else if command == BAR_THICKNESS {
        Some(BAR_THICKNESS_HELP)
    } else if command == STYLE {
        Some(STYLE_HELP)
    } else if command == SHOW_PALETTES {
        Some(SHOW_PALETTES_HELP)
    } else if command == TUNE_PALETTES {
        Some(TUNE_PALETTES_HELP)
    } else if command == LOG {
        Some(LOG_HELP)
    } else if command == COMPRARE_LEVEL {
        Some(COMPRARE_LEVEL_HELP)
    } else if command == SAVE {
        Some(SAVE_HELP)
    } else if command == LOAD {
        Some(LOAD_HELP)
    } else if command == CHANGELOG {
        Some(CHANGELOG_HELP)
    } else if command == SHOW_LANGUAGES {
        Some(SHOW_LANGUAGES_HELP)
    } else if command == SHOW_CONFIGS {
        Some(SHOW_CONFIGS_HELP)
    } else {
        None
    }
}
