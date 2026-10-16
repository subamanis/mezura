-- mezura-expect lines=23 code=12 comments=8 extra=3 functions=2
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
local text = [[raw text across
two lines, no escapes]]
local deep = [==[ text with ]] inside
still the string ]==]
