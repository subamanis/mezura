// mezura-expect lines=12 code=8 comments=2 extra=2 structs=1 enums=1
const s = "// not a comment";
const brace = '{';
const lines =
    \\// not a comment
    \\struct NotCounted
;

// a comment
const Point = struct { x: i32 };
const Colour = enum { red };
var n: i32 = 1;  // trailing
