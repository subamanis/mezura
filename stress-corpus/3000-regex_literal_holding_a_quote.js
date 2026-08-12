// mezura-real  10 lines 2 code 8 comment
// mezura-count 10 lines 2 code 8 comment
// tokei-real   10 lines 2 code 8 comment
// tokei-count  10 lines 3 code 7 comment
// trap: a quote inside a regex literal, which costs its own line at most where the language
// declares its quotes as ending with the line
// tokei: the quote opens a string that runs on, so the comment under it is counted as code
let re = /"/;
let x = 1;
// a real comment
