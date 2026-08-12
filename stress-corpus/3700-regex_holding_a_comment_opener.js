// mezura-real  11 lines 3 code 8 comment
// mezura-count 11 lines 1 code 10 comment
// tokei-real   11 lines 3 code 8 comment
// tokei-count  11 lines 1 code 10 comment
// trap: a comment opener inside a regex literal
// mezura: the /* opens a comment and swallows the lines under it. Telling a regex from a division
// needs the token before the slash, which is a lexer's job
// tokei: the same, the /* opens a comment and the lines under it are counted as comment
let re = /a[/*]b/;
let x = 1;
let y = 2;
