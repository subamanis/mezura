-- mezura-real  9 lines 3 code 6 comment
-- mezura-count 9 lines 3 code 6 comment
-- tokei-real   9 lines 3 code 6 comment
-- tokei-count  9 lines 2 code 7 comment
-- trap: Lua writes a string with the same long brackets its comments use, so the -- inside one is text
-- tokei: its long bracket handling covers the comment form and not the string one, so the -- opens a comment
local s = [[a string
-- not a comment, it is inside the string
]]
