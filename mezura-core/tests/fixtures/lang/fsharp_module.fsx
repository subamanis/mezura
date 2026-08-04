// mezura-expect lines=9 code=4 comments=3 extra=2 modules=1 types=1
module Greeter

(* a block
   comment *)
type Person = { Name : string }

let greet (p: Person) =
    printfn "hello %s" p.Name
