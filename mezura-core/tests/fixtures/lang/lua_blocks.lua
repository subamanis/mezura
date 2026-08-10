-- mezura-expect lines=18 code=7 comments=9 extra=2 functions=2
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
