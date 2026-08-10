// mezura-real  8 lines 3 code 5 comment
// mezura-count 8 lines 1 code 7 comment
// trap: a comment opener inside a regex literal
// mezura: the /* opens a comment and swallows the lines under it. Telling a regex from a division
// needs the token before the slash, which is a lexer's job
let re = /a[/*]b/;
let x = 1;
let y = 2;
