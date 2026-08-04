(* mezura-expect lines=11 code=5 comments=4 extra=2 types=1 modules=1
*)
module Greeter = struct
  type t = { name : string }

  (* a comment
     over two lines *)
  let make name = { name }
end

let () = print_string "hi"
