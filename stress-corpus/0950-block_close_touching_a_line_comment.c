// mezura-real  11 lines 1 code 10 comment
// mezura-count 11 lines 1 code 10 comment
// tokei-real   11 lines 1 code 10 comment
// tokei-count  11 lines 2 code 9 comment
// trap: the '//' opening the line comment is formed on the '/' that closes the block comment, so
// the two share a byte and no search for '//' yields both of the places it appears to be at. Case
// 0550 is the same line with a space between them, and Pascal's '{ block }//trailing' is the same
// line again with a closer that shares no byte with it
// tokei: the line counts as code, for the reason case 0550 gives, and the shared byte adds nothing
/* block *///trailing
int x = 1;
