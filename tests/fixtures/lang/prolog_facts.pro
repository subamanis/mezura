% mezura-expect lines=8 code=4 comments=3 extra=1
greeting(hello).

/* a block
   comment */
greet(Name) :-
    greeting(G),
    format("~w ~w~n", [G, Name]).
