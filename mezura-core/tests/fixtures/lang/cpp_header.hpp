// mezura-expect lines=16 code=8 comments=5 extra=3 classes=1 structs=1
#pragma once

/* a block
   comment */
class Widget {
    struct Inner { int a; };
};
enum struct Mode : int { A, B };

// a comment continued \
   onto the next line
const char *s = "// not a comment";
const char *raw = R"(a raw string)";
const char *tagged = R"x(a " /* b)x";
int after = 1;
