-- mezura-expect lines=10 code=5 comments=3 extra=2 classes=1 types=1
module Main where

{- a block
   comment -}
data Colour = Red | Green

class Show a where
  render :: a -> String
greeting = "-- not a comment" ++ ['x']
