; mezura-expect lines=6 code=2 comments=3 extra=1 definitions=2
#| outer #| inner |# still outer |#
(define x "; not a comment")

; a comment
(define (square n) (* n n))
