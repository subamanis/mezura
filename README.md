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
* [Configuration Files](#configuration-files)
* [Logs and Progress](#logs-and-progress)
* [Color Palettes](#color-palettes)
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
- Difference of stats between executions 

By default, the files and folders that are ignored by a .gitignore are skipped, so that build artifacts and dependencies don't pollute the stats (see the ```--no-gitignore``` command).

There is a "data" folder in the repository, that contains some already provided language files, color palettes and the default configuration file.
The program, at compile time, includes the "data" folder in the binary, and during the first execution, it saves it with the same structure in a persistent path, inside the user's computer, according to the platform's specification. More specifically, the paths per operating system are:
```
    Windows:  %APPDATA%\mezura
    Linux:    /home/$USER/.local/share/mezura
    MacOs:    /Users/$USER/Library/Application Support/mezura
```

After every subsequent execution, the languages, color palettes, configurations and logs, are read from these folders, so the user can have easy access and modify them,
like add more languages of his choice, add custom color palettes, or modify the default configuration.

In order for a file to be considered for counting, its extension must be supported, meaning that a .txt language file specifying the particular extension as an entry in its 'Extensions' field, must be present in the "data/languages" dir, see [Supported Languages](#supported-languages).


## Cmd Commands
Below there is a list with all the commands-flags that the program accepts.
```
--help

    No arguments or any number of existing other commands.

    Overrides normal program execution and just displays this message on the terminal.
    If more commands are provided, information will be displayed specifically about them.

--changelog

    No arguments, or the optional argument 'full'.

    Overrides normal program execution and just prints a summary of the changes of the current version of
    the program. If 'full' is provided, the changes of every previous version are printed too,
    most recent first.

--show-languages

    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of all the supported
    languages that were detected in the persistent data path of the application, where you can add more.

--show-configs

    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of all the configuration
    files that were detected in the persistent data path of the application. 

--show-palettes

    No arguments, or one of 'slim', 'medium', 'fat' and 'low'. Default: slim

    Overrides normal program execution and just prints a sorted list with the names of all the color palettes
    that were detected in the persistent data path of the application, where you can add more,
    each one previewed on a mock overview.

    The optional argument draws the preview bars with the character that the '--bar-thickness' command
    would use, so that a palette can be judged the way it will actually be printed.

--tune-palettes

    No arguments.

    Overrides normal program execution: generates an interactive HTML page with all the color
    palettes found in the persistent data path of the application, and opens it in the default
    browser. There, every color of every palette can be adjusted, with live contrast metrics
    and a mock overview, and the result is turned into a ready '--colors' command that you can
    use directly or save in a palette file.

--dirs

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
    <path1>`, <path2>`, <path3>   or   "<path1>, <path2>, <path3>"

    The target directories can also be given implicitly (in which case this command is not needed)
    with 2 ways:
    1) as the first arguments of the program directly
    2) if they are present in a configuration file (see [Configuration Files](#configuration-files)).

--exclude

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

    1..n arguments separated by commas, case-insensitive

    The given language names must exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    Only the languages specified here will be taken into account for the stats.

--exclude-languages

    1..n arguments separated by commas, case-insensitive

    The given language names should exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    The given language names will be ignored from the stats calculation, if they exist.

--threads

    2 numbers: the first between 1 and 8 and the second between 1 and 30. 

    This represents the number of the producers (threads that will traverse the given directories),
    and consumers (threads that will parse whatever files the producers found).

    If this command is not provided, the numbers will be chosen based on the available threads
    on your machine. Generally, a good ratio of producers-consumers is 1:3
    
--braces-as-code

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Specifies whether lines that carry no content should be considered as code lines or not.
    A line carries no content when nothing but punctuation is left on it once the strings and
    the comments are taken out: '}', '});', '],', ')'.

    The default behaviour is to not count them as code, since it is silly for code of the same content
    and substance to be counted differently, according to the programmer's code style.
    Writing 'if (x) { do(); }' on one line or on three should not change the number.
    This helps to keep the stats clean when using code lines as a complexity and productivity metric.

    Note that other line counters count these lines as code, so this is where their number and
    mezura's differ. Enabling this flag makes the two closer.

--search-in-dotted

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Specifies whether the program should traverse directories that are prefixed with a dot,
    like .vscode or .git.

--show-faulty-files

    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
    or while reading its contents. The default behavior when this happens is to count all of
    the faulty files and display their count.

    Specifies that their path, along with information about the exact error is displayed too.
    The most common reason for this error is if a file contains non UTF-8 characters. 

--no-gitignore

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

--hide

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

--colors

    1 to 5 colors separated by spaces. A color is either a hex value, with or without a leading
    '#' (e.g. ff8800 #00ff00), or one of the 16 standard terminal color names (black, red, green,
    yellow, blue, magenta, cyan, white and their bright- variants, e.g. bright-magenta).

    Overrides the colors used in the "overview" section of the results: the first three colors
    are used for the three most relevant languages, the fourth for a fourth language, and the
    optional fifth for the 'others' entry (if omitted, the fourth is used for 'others' too).
    If fewer colors are provided, the remaining ones keep their default color.

    Named colors follow the terminal's color scheme; hex colors are rendered with 24-bit ANSI
    codes, so the terminal must support truecolor.
    If you are using Windows Powershell, either omit the '#' or surround the color with
    quotation marks ("#ff8800"), since an unquoted '#' starts a comment.

--color-palette

    One argument, the name of a palette (case-insensitive).

    Applies a named color palette. Palettes are .txt files in the 'data/palettes' dir, in the
    persistent data path of the application, where the file name is the palette name. Each line
    is a 'token = value' pair: 'languages' takes 1 to 5 colors in the same format as the
    '--colors' command, and any style token (see '--style') takes a style.
    You can add your own palettes there.
    Use the '--show-palettes' command to list the available palettes.

    If the '--colors' command is also provided, it takes precedence over the palette's
    'languages' line, while the palette's style tokens still apply.
    Palettes written for versions before v3.0.0 are converted automatically on the first run.

--sort

    One argument: 'lines', 'files', 'code', 'size' or 'name'. Default: lines

    Chooses the order of the languages in the "details" section, which also decides which of them
    reach the "overview" section and which are folded into its 'others' entry.
    Every criterion except 'name' sorts from the largest down, and ties are broken alphabetically
    so that the order never changes between runs on the same data.

    Note that before v3.0.0 the order was a fixed formula in which the byte size dominated, so
    runs that used to be ordered by size are now ordered by lines unless you ask otherwise.

--top

    One number, 1 or greater.

    Shows only that many languages in the "details" section, the ones that come first according
    to the '--sort' criterion. A line underneath states how many were hidden, so that the numbers
    not adding up to the total is never a mystery.

    The total keeps counting every language, hidden ones included, since it is the total.
    The "overview" section never shows more languages than this either, so asking for the top 2
    does not leave a third one sitting in the bar.

--bar-thickness

    One argument: 'slim', 'medium', 'fat' or 'low'. Default: slim

    Chooses the character that the percentage bar of the "overview" section is drawn with.

      slim     |   the default, and the only one made of ASCII, so it is the one that is
                   guaranteed to render on every terminal
      medium   ┃   thicker, but still leaves gaps between the strokes
      fat      █   fills the cell, so the boundary between two language colors is crisp
      low      ▄   fills only the bottom of the cell, a thin band under the text

    All but 'slim' need a terminal and a font that can render box drawing characters.
    If the bar comes out as question marks or empty boxes, use 'slim'.

--style

    One or more 'token=style' pairs separated by commas, for example:
    --style number=bright-black,label=b5a98a italic,heading=white bold underline

    Overrides how a category of printed text looks. A style is a color and any number of the
    attributes 'bold', 'italic', 'underline', 'dim' and 'reverse', in any order. The color is
    either a hex value or one of the 16 terminal color names (the same grammar as '--colors'),
    or the word 'default' to leave the terminal's own foreground color alone.

    'reverse' swaps the text and background colors, so it stands out strongly without committing
    to any color of its own, which means it adapts to whatever theme the terminal is using.

    The tokens are:
      heading          the section titles and the "Analyzing directories" lines
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

--save

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Doing so, will run the program and also create a .txt configuration file,
    inside 'data/config/' with the specified name, that can later be loaded with the --load command.

--load

    One argument as the file name (whitespace allowed, without an extension, case-insensitive)
    
    Associated with the '--save' command, this command is used to load the flags of 
    an existing configuration file from the 'data/config/' directory. 

    You can combine the '--load' and '--save' commands to modify a configuration file.

--log

    0..n words as arguments in the cmd.
    If specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    This flag only works if a configuration file is loaded. Specifies that a new log entry should be made
    with the stats of this program execution, inside the appropriate file in the 'data/logs' directory.
    If not log file exists for this configuration, one is created.
    All the provided arguments are used as a description of the log entry.

    This flag will not be saved in a configuration file automatically, but it can be added manually.

--compare

    1 argument: a number between 0 and 10. Default: 1

    This flag only works if a configuration file is loaded. Specifies with how many previous logs this
    program execution should be compared to (see [Logs and Progress](#logs-and-progress)).

    Providing 0 as argument will disable the progress report (comparison).
```


## Configuration Files
If we plan to run the program many times for a project, it can be bothersome to specify all the flags every time, especially if they contain a lot of target and exclude dirs.
That's why you can specify many flags in a <b>*configuration file*</b>, and have the program just load that file (see the --load command). <br>

Configurations can be created automatically by specifying all the flags once, along with the command "--save", and a name for the configuration. Then the program, along with its normal execution, will automatically create a config file with the name you specified, and dump all the flags in there. <br>
The next time you want to run the program on this project, you can do it like this: 
```mezura --load <config_name>``` <br>

By default, there is a configuration file name "default" already present in the "data/config" dir, that gets loaded on every run. There, you can customize your preferences and they will apply to all runs, except if overriden by explicitely providing a different flag in the cmd, or by loading a specific configuration. For example, if you prefer counting braces as code, you can specify it there, because the default behaviour is to not regard them as code. <br>

The priorities of the specified flags are:
1) cmd
2) Specific config file
3) Default config file
4) Internal defaults



## Logs and Progress
Inside the 'data/logs' folder, the program will save log files that correspond to saved configurations everytime the '--log' flag is used. <br>
Inside the log files, the date and time of the execution and the name of the log (if specified) are saved, along with information about the current configuration (like the target directories, whether braces should be considered code, etc, so you can see if at some point the configuration got modified), and also the total files, lines, code lines,
comment lines, extra lines, size and average size of the execution. They are in an easy to parse format for external use also. <br>

Entries written before v3.0.0 have no comment count, and their "Extra" included the comments. Such an entry is recognised and its extra lines are printed without a comparison, instead of reporting a drop that never happened. <br>

By using the '--compare <N>' flag, the (N) previous logged executions will be retrieved from the file and will be compared and printed to the screen. For example
for N = 3, it would look like this:
![](screenshots/compare-logs.PNG)

Note that a configuration file must be loaded for both '--log' and '--compare' to work.



## Color Palettes
The colors of the "overview" section can be customized, either by giving explicit colors with the ```--colors``` command, or by applying a named palette with the ```--color-palette``` command. A color is either a hex value or one of the 16 standard terminal color names, which follow the color scheme of your terminal.

8 palettes are bundled: <b>Mezura</b> (the default one), Dracula, Nord, Gruvbox, Catppuccin, Sunset, Neon and Ocean. They are plain .txt files in the "data/palettes" dir of the persistent data path, so you can edit them, or add your own by dropping a file there. Use the ```--show-palettes``` command to list the ones that were found.

To make picking colors easier, there is an interactive palette tuner, where every palette is previewed on a mock overview, every color can be adjusted (with live contrast and color distance metrics, so that the result stays readable), and the outcome is turned into a ready to use command.

<b>[Open the palette tuner online](https://subamanis.github.io/mezura/palette-tuner/)</b> to play with the bundled palettes, or run ```mezura --tune-palettes``` to open it with the palettes found on your own machine, including the ones you created.

<a href="https://subamanis.github.io/mezura/palette-tuner/"><img src="screenshots/palette-tuner.png" width="900"></a>



## Supported Languages
Note that the default supported languages are incomplete, but they can be easily expanded by the user. All the supported languages can be found in the folder "data/languages"
as separate text files, in the persistent data path of the application. 
The user can easily specify a new language by replicating the format of the language files and customizing it accordingly, either by following the rules below or by copy pasting an existing file.

Header files have their own dedicated languages: `.h` files are counted under "C Header" and `.hpp` files under "C++ Header", since the program cannot know which codebase a header belongs to. If two or more language files claim the same extension, all files with this extension are counted under the language that comes first alphabetically.

The format of the languages is as follows(and should not be modified at all):

```
Language
<name of the language>

Extensions
<name of file extensions like cpp hpp or py, separated by whitespace>

String symbols
<either 1 or 2 string symbols, separated by whitespace, like: " ' >

Comment symbols
<either 1 or 2 single line comment symbols, separated by whitespace, like: // # >

```
All the following lines are optional and can be omitted. You can also specify an arbitrary amount of keywords.
```
Multiline comment start symbol
<a symbol like: /*>

Multiline comment end symbol
<a symbol like: */>

Keyword
    NAME
    <the name of the keyword to be shown in the results, like: classes>
    ALIASES
    <any word that constitutes an instance of this keyword, like: class, record>
Keyword
    NAME
    <the name of the keyword to be shown in the results, like: classes>
    ALIASES
    <any word that constitutes an instance of this keyword, like: class, record>
```

	
## Accuracy and Limitations
The program is able to understand and parse correctly arbitrarily complex code structures with intertwined strings and comments. This way it can identify if a line contains something other than a comment, even if the comment is partitioned in multiple positions and it can identify valid keywords, that are not inside strings or comments.
For example in a line like ```/*class"*/" class" aclass```, it will not count "class" as a keyword since the first is inside a comment, the second inside a string and the third has a prefix.
Additionally:
- It checks for escaped characters, for example ```/"``` will not be counted as a string symbol
- It resolves symbols that are side by side, for example ```*/*``` is normally identified as both a closing and an opening comment symbol, but the program will understand the correct usage.

With that said, it is important to mention the following limitations:

- The program cannot understand language specific syntax or details, this would require a handwritten, complex, language-specific parser for most different languages. For example, in a .php file that contains html or js, the distinction will not be made. Also, the keyword counting doesn't take any measures to ensure that a valid keyword has the user-intended meaning. For example, the word "class" may appear in the syntax of a programming language with an additional use than declaring a class. This may lead to some false positives.

- If a target path contains another target path, the contained one is dropped, so that its files are not counted twice. The program does not detect duplicates in any other case though: if a directory that is being scanned contains a symbolic link (or a Windows junction) that points to another directory, the files of that directory are counted a second time through the link, and the same is true for hard links to the same file.

- Glob patterns (* ? [..] {..}) are supported both by the target paths (```--dirs```) and by the ```--exclude``` command, but full regular expressions are not supported anywhere.

- The program assumes that if a line contains any odd number of the same string symbols, then this is an open multiline string. This works for most cases but it may create inaccuracies, for example if a line in python has """ then the program will consider a multiline string everything until the next " symbol and not the next """ symbol. If a language doesn't support multiline strings, then you would not expect to see odd number of string symbols either way in a valid syntax.

- A language can only declare either one or two string and comment symbols and only one multiline comment start symbol + multiline comment end symbol in the .txt, not more.

- Regural expressions are not handled in a special way, so if a regex contains a string or comment symbol, it may create some inaccurancies for the file.

- Bug: If a file contains Unicode Strings, there is a possibility that a parser thread will panic, due to trying to slice a line in a non-valid way, thus creating
non-valid unicode characters. (byte index is not a char boundary).  Bulletproof Unicode parsing is a very tricky problem that is not worth it to implement, but the existing behavior should work for 99.9% of files.


## Windows Performance Note

Opening a file is far more expensive on Windows than on Linux: every open walks the object manager, the security descriptor and the whole filter driver stack, which is where antivirus and other minifilters sit. Since mezura opens one file after another, this dominates: **on Windows the program is I/O bound, and most of its time is spent waiting on `File::open` rather than counting anything**. On Linux the same open is cheap, so the program is parsing bound and the time goes where it should, into the parser.

The practical consequence is that the same repository on the same machine is measurably faster to analyze from Linux (~2x speedup), and that on Windows the biggest wins come from removing work from the open path rather than from making the parser faster.

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


## Similar Projects

If you don't require the keyword counting functionality of this program, the progress tracking feature, or the alternate-than-usual visualization, use the [scc](https://github.com/boyter/scc) project written in GO, that is honestly impressive.

Other alternative projects you can check are:
- [loc](https://github.com/cgag/loc)
- [cloc](https://github.com/AlDanial/cloc)
- [sloc](https://github.com/flosse/sloc)
- [tokei](https://github.com/XAMPPRocky/tokei)
