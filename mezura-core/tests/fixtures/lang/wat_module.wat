;; mezura-expect lines=8 code=4 comments=3 extra=1 functions=1 modules=1
(module
  (; a block
     comment ;)
  (func $greet (result i32)
    i32.const 42)
  (data (i32.const 0) ";; not a comment")
)
