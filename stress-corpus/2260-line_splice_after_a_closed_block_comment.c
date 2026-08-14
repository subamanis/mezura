// mezura-real  12 lines 2 code 10 comment
// mezura-count 12 lines 2 code 10 comment
// tokei-real   12 lines 2 code 10 comment
// tokei-count  12 lines 4 code 8 comment
// trap: a block comment closes and a line comment opens on the same line, with a space between
// the two. That space is not code, so the splice ending the line still carries the comment on
// tokei: two faults on one line, so it loses two. The line reads as code, which case 0550 holds
// on its own, and the splice does not carry, which case 2200 holds
int a = 1;
/* block */ // trailing \
int x = 1;
int y = 2;
