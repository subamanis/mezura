% mezura-expect lines=9 code=5 comments=3 extra=1
greeting(hello).

/* a block
   comment */
greet(Name) :-
    greeting(G),
    format("~w ~w~n", [G, Name]).
label('% not a comment').
