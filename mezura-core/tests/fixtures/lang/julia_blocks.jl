# mezura-expect lines=14 code=8 comments=3 extra=3 structs=1 modules=1
module Greetings

#= a block
   comment =#
struct Person
    name::String
end

function greet(p::Person)
    println("hello ", p.name)
end

end
