// mezura-real  14 lines 2 code 9 comment
// mezura-count 14 lines 2 code 9 comment
// tokei-real   14 lines 4 code 10 comment
// tokei-count  14 lines 2 code 12 comment
// trap: '//*/' closes a block comment, because '//' means nothing inside one. It is the C idiom
// for switching a block off and on again by adding one character in front of its closer
// tokei: the block is never closed, so the rest of the file counts as comment
class Kept
{
    /* Keep it for reference
    int old = 1;
    //*/
    int kept = 2;
}
