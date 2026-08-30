" mezura-expect lines=7 code=6 comments=1 extra=0 functions=1
set laststatus=2
let s:name = "mezura"
let s:quoted = 'a " quote'
function! s:Greet() abort
  echo s:name
endfunction
