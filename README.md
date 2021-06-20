# code_stats

## About
This is a fast, customizable and fairly accurate stats generator for programming projects, in the form of a CLI executable, written in Rust, with minimal dependencies. It is used for counting total lines, code lines, and user defined keywords like classes.

## Table of contents
* [How To Run](#how-to-run)
* [Supported Languages](#supported-languages)
* [Details](#details)
* [Performance](#performance)
* [Limitations](#limitations)


## How To Run
You can run the project directly by dowloading the "target/release" folder that contains the executable and the neccessary "data" folder.
Alternatively, you can build the project yourself "cargo b --release".

The program, expects a path to a directory or a code file, that can be provided as cmd argument, or if not, you will be prompted to provide it after running the program.
The program also accepts a lot of optional flags to customize functionality, see the 
[Details](#details) section for more info or use the --help command.


## Supported Languages
All the supported languages can be found in the folder "data/extensions" as seperate text files. 
The user can easily specify a new language by replicating the format of the extension files and customizing it accordingly, either by following the rules below or by copy pasting an existing file.

The format of the extensions is as follows(and should not be modified at all):

```
Extension
<name of file extension like java or py>

String symbols
<either 1 or two string symbols seperated by space, like: " ' >

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
    name
    <the name of the keyword to be shown in the results, like: classes>
    aliases
    <any word that constitutes an instance of this keyword, like: class, record>
Keyword
    name
    <the name of the keyword to be shown in the results, like: classes>
    aliases
    <any word that constitutes an instance of this keyword, like: class, record>
```
	
## Details
The program requires a "data" dir to be present on the same level as the executable(or 2 levels up in the folder hierarchy). In the "data" dir an "extensions" dir must be present, that contains the supported extensions. An optional "config" dir may be present too, where the user can specify persistent settings, more on that later.

The program counts the lines of files in the specified directory. In order for a file to be considered for counting, it's extension must be supported, meaning that a .txt file specifying the details of the extension must be present in the "data/extensions" dir see [Supported Languages](#supported_languages). 

The program distinguishes the total lines in code lines and "extra" lines (all the lines that are not code).<b>Note</b> that braces "{ }" are not considered as code by default, but this can be changed by using the --braces-as-code flag.
Also, the program can search for user-defined keyword that are specified in the extensions files and count their occurances. 

The program can identify keywords in complex lines correctly, meaning that it will check whether the keyword is inside a comment, a string, if it has a prefix or suffix and will not consider it.

Below there is a list with all the commands-flags that the program accepts.

## Performance
...
	
## Limitations
To run this project, install it locally using npm:
