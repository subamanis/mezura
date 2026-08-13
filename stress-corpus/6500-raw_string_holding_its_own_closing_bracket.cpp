// mezura-real  11 lines 5 code 6 comment
// mezura-count 11 lines 5 code 6 comment
// tokei-real   11 lines 5 code 6 comment
// tokei-count  11 lines 5 code 6 comment
// trap: the opening line closes its quotes, so only the raw pair itself keeps the block opener
// under it from starting a comment that never ends. A lone ) inside does not end the string
const char *raw = R"(a lone " quote and
/* this opens nothing, it is inside the raw string
still inside, and a lone ) is not the end
)";
printf(")");
