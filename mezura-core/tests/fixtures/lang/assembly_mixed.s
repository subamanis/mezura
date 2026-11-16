// mezura-expect lines=16 code=10 comments=5 extra=1
#include "macros.h"
.globl main

; a NASM style comment
# a GAS on ARM comment
/* a block
   comment */
main:
    mov eax, 1
#ifdef DEBUG
    nop
#endif
    ret
    .asciz "// not a comment"
    .byte 'a'
