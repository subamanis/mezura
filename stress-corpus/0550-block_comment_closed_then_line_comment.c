// mezura-real  10 lines 1 code 9 comment
// mezura-count 10 lines 1 code 9 comment
// tokei-real   10 lines 1 code 9 comment
// tokei-count  10 lines 2 code 8 comment
// trap: a block comment opens and closes on one line and a line comment follows it. Nothing is
// outside the two but the space between them, so the line is comment and holds no code
// tokei: the line counts as code. The same line comment after a block that spans lines, after two
// blocks, or after nothing at all is counted correctly, so it is this shape alone
/* block */ // trailing
int x = 1;
