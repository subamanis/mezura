// mezura-real  10 lines 2 code 7 comment
// mezura-count 10 lines 2 code 7 comment
// tokei-real   10 lines 2 code 8 comment
// tokei-count  10 lines 1 code 9 comment
// trap: a block closes, code follows, then a stray close and a real open share a byte
// tokei: a line that began inside a block is a comment, so the statement after the close is lost
int w = 8; /* tail
*/ int z = 9; */* int y = 10;
hidden by the reopened block
*/
