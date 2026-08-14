-- mezura-expect lines=19 code=8 comments=8 extra=3 functions=2
local greeting = "hello"

--[[ this block
     spans three lines
     and ends here ]]
--[[ closes and reopens
]] local x = 1 --[[ code between blocks
     still a comment ]]
function greet(name)
    print(greeting .. name)
end

function bye()
end
--[==[ a level two block
with ]] inside as text
]==]
local s = '-- not a comment'
