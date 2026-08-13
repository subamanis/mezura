-- mezura-expect lines=15 code=9 comments=4 extra=2 types=1
module Main exposing (main)

{- a block comment
   over two lines -}
type Greeting
    = Hello
    | Bye

-- a line comment
main = Hello
note = "-- not a comment"
doc = """
a block string
"""
