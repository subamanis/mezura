-- mezura-expect lines=12 code=6 comments=4 extra=2 functions=2
local greeting = "hello"

--[[ this block
     spans three lines
     and ends here ]]
function greet(name)
    print(greeting .. name)
end

function bye()
end
