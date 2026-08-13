-- mezura-real  10 lines 3 code 7 comment
-- mezura-count 10 lines 1 code 8 comment
-- tokei-real   10 lines 3 code 7 comment
-- tokei-count  10 lines 2 code 8 comment
-- trap: the [[ ]] string form is not declared, so a -- written inside one opens a comment
-- mezura: the body is read as ordinary text, so the line inside the string counts as a comment
-- tokei: the same, its long bracket handling covers the comment form and not the string one
local s = [[a string
-- not a comment, it is inside the string
]]
