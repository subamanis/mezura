// mezura-expect lines=12 code=6 comments=5 extra=1
.globl main

; a NASM style comment
# a GAS on ARM comment
/* a block
   comment */
main:
    mov eax, 1
    ret
    .asciz "// not a comment"
    .byte 'a'
