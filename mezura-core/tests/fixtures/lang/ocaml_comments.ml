(* mezura-expect lines=14 code=6 comments=5 extra=3 types=1 modules=1
*)
module Greeter = struct
  type t = { name : string }

  (* a comment
     over two lines *)
  (* nested (* inner *) still
     comment here *)
  let make name = { name }
end

let () = print_string "hi"
let initial = 'x'
