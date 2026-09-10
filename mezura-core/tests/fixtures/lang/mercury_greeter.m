% mezura-expect lines=46 code=29 comments=4 extra=13 types=1 predicates=2 functions=2 language=Mercury

:- module greeter.
:- interface.

:- import_module char.
:- import_module io.
:- import_module list.

    % Greets everybody on the list, one name per line.
:- pred greet(list(string)::in, io::di, io::uo) is det.

:- implementation.

:- import_module string.

:- type greeting
    --->    plain(string)
    ;       loud(string).

:- func render(greeting) = string.

render(plain(Name)) = "hello, " ++ Name.
render(loud(Name)) = to_upper("hello, " ++ Name) ++ bang.

:- func bang = string.

bang = char_to_string('!').

greet([], !IO).
greet([Name | Names], !IO) :-
    io.write_string(render(loud(Name)), !IO),
    io.nl(!IO),
    greet(Names, !IO).

/* A block comment, which Mercury has beside the line kind,
   and which does not nest. */
:- pred beep(io::di, io::uo) is det.

:- pragma foreign_proc("C",
    beep(IO0::di, IO::uo),
    [will_not_call_mercury, promise_pure],
"
    putchar(0x07);
    IO = IO0;
").
