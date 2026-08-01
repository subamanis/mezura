% mezura-expect lines=9 code=4 comments=3 extra=2 modules=1
-module(greeter).
-export([greet/1]).

% greets somebody
greet(Name) ->
    io:format("hello ~s~n", [Name]).

% done
