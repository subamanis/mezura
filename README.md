# mezura

## About
This is a <b>fast</b>, <b>customizable</b> and fairly <b>accurate</b> stats generator for programming projects, in the form of a CLI executable, written in <b>Rust</b>, with <b>minimal dependencies</b>. It is used for counting total lines, code lines, and user defined <b>keywords</b> like classes.

Example run:
![](screenshots/example.PNG)


## Table of contents
* [How To Run](#how-to-run)
* [Details](#details)
* [Supported Languages](#supported-languages)
* [Accuracy and Limitations](#accuracy-and-limitations)
* [Performance](#performance)
* [Similar Projects](#similar-projects)



## How To Run
You can run the project directly by dowloading the "executable/release" folder that contains the executable and the neccessary "data" folder.
Alternatively, you can build the project yourself ```cargo b --release```

Format of arguments: ```<path> --optional_command1 --optional_commandN```

The program, expects a path to a directory or a code file (they can be many different ones, seperated by comma), that can be provided as cmd argument, or if not, you will be prompted to provide it after running the program.
The program also accepts a lot of optional flags to customize functionality, see the next section for more info or use the --help command.

	
## Details
The generated stats are the following:
- Number of files
- Lines (code + others) and percentages
- Size (total and average) 
- Keyword occurances
- Percentage comparisons between languages

The program requires a "data" dir to be present on the same level as the executable. In the "data" dir, a "languages" dir must be present, that contains the supported languages as seperate txt files. An optional "config" dir may be present too, where the user can specify persistent settings (more on that later).

The program counts the lines of files in the specified director(y/ies). In order for a file to be considered for counting, its extension must be supported, meaning that a .txt language file specifying the particular extension as an entry in its 'Extensions' field, must be present in the "data/languages" dir see [Supported Languages](#supported-languages). 

The program distinguishes the total lines in code lines and "extra" lines (all the lines that are not code).
<b>Note</b> that braces "{ }" are not considered as code by default, but this can be changed either by using the --braces-as-code flag during a particular run of the program, or enabling it in the "default" config file to use globally always.
Also, the program can search for user-defined <b>keywords</b> that are specified in the language files and count their occurances, while identifying them correctly in <b>complex lines</b>, see [Accuracy and Limitations](#accuracy-and-limitations) for details.

Below there is a list with all the commands-flags that the program accepts.
```
--help
    Display this message on the terminal. No other arguments or commands are required.
    
 --dirs
    The paths to the directories or files, seperated by commas if more than 1,
    in this form: '--dirs <path1, path2>'
    They can either be surrounded by quotes: \"path\" or not, even if the paths have whitespace.

    The target directories can also be given implicitly (in which case this command is not needed) with 2 ways:
    1) as the first arguments of the program directly
    2) if they are present in a configuration file (see '--save' and '--load' commands).

--exclude 
    1..n arguments separated by commas, can be a folder name, a file name (including extension), 
    or a full path to a folder or file. The paths can be surrounded by quotes or not,
    even if they have whitespace.

    The program will ignore these dirs.

--languages 
    1..n arguments separated by commas, case-insensitive

    The given language names must exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    Only the languages specified here will be taken into account for the stats.

--threads
    1 argument: a number between 1 and 8. Default: 4 

    This reprisents the number of the consumer threads that parse files,
    there is also always one producer thread that is traversing the given dir.

    Increasing the number of consumers can help performance a bit in a situation where
    there are a lot of big files, concentrated in a shallow directory structure.
    
--braces-as-code
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or anything else to disable. Default: disabled

    Specifies whether lines that only contain braces, should be considered as code lines or not.

    The default behaviour is to not count them as code, since it is silly for code of the same content
    and substance to be counted differently, according to the programer's code style.
    This helps to keep the stats clean when using code lines as a complexity and productivity metric.

--search-in-dotted
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or anything else to disable. Default: disabled

    Specifies whether the program should traverse directories that are prefixed with a dot,
    like .vscode or .git.

--show-faulty-files
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or anything else to disable. Default: disabled

    Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
    or while reading it's contents. The default behavior when this happens is to count all of
    the faulty files and display their count.

    This flag specifies that their path, along with information about the exact error is displayed too.
    The most common reason for this error is if a file contains non UTF-8 characters. 

--no-visual
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or anything else to disable. Default: disabled

    Disables the colors in the "overview" section of the results, and disables the visualization with 
    the vertical lines that reprisent the percentages.

--save
    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    If we plan to run the program many times for a project, it can be bothersome to specify,
    all the flags every time, especially if they contain a lot of target and exclude dirs for example.
    That's why you can specify all the flags once, and add this command to save them
    as a configuration file. If you specify a '--dirs' command, it will save the absolute
    version of the specified path in the config file, otherwise, no path will be specified.

    Doing so, will run the program and also create a .txt configuration file,
    inside 'data/config/' with the specified name, that can later be loaded with the --load command.

--load
    One argument as the file name (whitespace allowed, without an extension, case-insensitive)
    
    Assosiated with the '--save' command, this command is used to load the flags of 
    an existing configuration file from the 'data/config/' directory. 

    There is already a configuration file named 'default.txt' that contains the default of the program,
    and gets automatically loaded with each program run. You can modify it to add common flags
    so you dont have to create the same configurations for different projects.

    If you provide in the cmd a flag that exists also in the specified config file,
    then the value of the cmd is used. The priority is cmd> custom config> default config. 
    You can combine the '--load' and '--save' commands to modify a configuration file.
```


## Supported Languages
All the supported languages can be found in the folder "data/languages" as seperate text files. 
The user can easily specify a new language by replicating the format of the language files and customizing it accordingly, either by following the rules below or by copy pasting an existing file.

The format of the languages is as follows(and should not be modified at all):

```
Language
<name of the language>

Extensions
<name of file extensions like cpp hpp or py, seperated by whitespace>

String symbols
<either 1 or 2 string symbols seperated by whitespace, like: " ' >

Comment symbol
<single line comment symbol like: //>

```
all the following lines are optional and can be omitted
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

- The program cannot understand language specific syntax or details, this would require a handwritten, complex, language-specific parser for most different languages. For example, in a .php file that contains html or js, the destinction will not be made. Also, the keyword counting doesn't take any measures to ensure that a valid keyword has the user-intended meaning. For example, the word "class" may appear in the syntax of a programming language with an additional use than declaring a class. This may lead to some false positives.

- The program assumes that if a line contains any odd number of the same string symbols, then this is an open multiline string. This works for most cases but it may create inaccuracies, for example if a line in python has """ then the program will consider a multiline string everything until the next " symbol and not the next """ symbol. If a language doesn't support multiline strings, then you would not expect to see odd number of string symbols either way in a valid syntnax.

- A language can only declare either one or two string symbols in the .txt, not more.

- The program doesn't take into account gitignore files, the unwanted dirs have to be added manually in a configuration file


## Performance
On a cold run, performance is mainly limited by how fast the producer thread can traverse the directory and find relevant files, so the consumers can parse them.

The performance will also vary depending on how deep and wide the directory structure is, how big the code files are and how many keywords are specified to be counted. 

Here are some metrics for both hot and cold executions on my laptop (i5-1035G1, 2 keywords per language):

1) reletively deep and wide directory with big files (6 consumers)
```
4,066 files - lines 5,625,944 - average size 75 KBs

Hot
 1.13 secs (Parsing: 3649 files/s | 5,050,219 lines/s)
Cold
 1.61 secs (Parsing: 2528 files/s | 3,498,721 lines/s)
```
2) relatively deep and wide directory with average to small files (4 consumers)
```
3,824 files - lines 793,751 - average size 8.7 KBs

Hot
 0.29 secs 
Cold
 1.23 secs (Parsing: 3106 files/s | 644,801 lines/s)
```
3) very very deep and wide directory, my entire drive (4 consumers)
```
32,078 files - lines 15,101,949 - average size 21 KBs 

Hot
 11.59 secs (Parsing: 2807 files/s | 1,317,336 lines/s)
Cold
 36.21 secs (Parsing: 891 files/s | 418,475 lines/s)
```


## Similar Projects

If you don't require the keyword counting functionality of this program and the alternate-than-usual visualization, use the [scc](https://github.com/boyter/scc) project written in GO, that is honestly impressive.

Other alternative projects you can check are:
- [loc](https://github.com/cgag/loc)
- [cloc](https://github.com/AlDanial/cloc)
- [sloc](https://github.com/flosse/sloc)
- [tokei](https://github.com/XAMPPRocky/tokei)
