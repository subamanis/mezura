% mezura-expect lines=10 code=5 comments=3 extra=2 modules=1
-module(greeter).
-export([greet/1]).

% greets somebody
greet(Name) ->
    io:format("hello ~s~n", [Name]).

% done
greet() -> '% not a comment'.
