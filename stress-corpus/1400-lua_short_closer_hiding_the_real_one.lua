-- mezura-real  9 lines 1 code 7 comment
-- mezura-count 9 lines 1 code 7 comment
-- tokei-real   9 lines 1 code 8 comment
-- tokei-count  9 lines 2 code 7 comment
-- trap: a bare ]] must not hide the ]=] that begins one byte inside it
-- tokei: the bare ]] ends the comment, so the rest of that line counts as code
--[=[ a level one comment
]]=]
x = 1
