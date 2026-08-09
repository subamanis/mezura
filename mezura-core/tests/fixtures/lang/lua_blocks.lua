-- mezura-expect lines=15 code=7 comments=6 extra=2 functions=2
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
