# mezura-expect lines=18 code=12 comments=3 extra=3 structs=1 modules=1
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
c = 'x'
doc = """
a block string
"""
