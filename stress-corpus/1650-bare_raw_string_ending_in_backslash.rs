// mezura-real  11 lines 1 code 10 comment
// mezura-count 11 lines 1 code 10 comment
// tokei-real   11 lines 1 code 10 comment
// tokei-count  11 lines 3 code 8 comment
// trap: the same closing backslash as case 1600, in the raw form that carries no hashes. The two
// forms end the same way, so a reader that handles one and not the other is reading the hashes
// and not the rule
// tokei: the string never closes, so both comments under it are counted as code
let path = r"C:\ends\in\backslash\";
// a real comment
// a second real comment
