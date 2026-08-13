*> mezura-expect lines=9 code=6 comments=2 extra=1 divisions=2 programs=1
IDENTIFICATION DIVISION.
PROGRAM-ID. GREETER.
PROCEDURE DIVISION.
    DISPLAY "hello".
    STOP RUN.

*> done
       DISPLAY '*> not a comment'.
