// mezura-real  11 lines 4 code 7 comment
// mezura-count 11 lines 1 code 10 comment
// tokei-real   11 lines 4 code 7 comment
// tokei-count  11 lines 1 code 10 comment
// trap: a delimited raw string names its own terminator, so no declared symbol can close it
// mezura: the plain R"( is declared and this is not it, so the block opener inside the string
// tokei: the same, its verbatim quote is R"( and the delimited form is not declared either
const char *raw = R"xy(a lone " quote and
/* this opens nothing, it is inside the raw string
)xy";
printf("done");
