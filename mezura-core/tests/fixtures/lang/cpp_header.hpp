// mezura-expect lines=13 code=5 comments=5 extra=3 classes=1 structs=1
#pragma once

/* a block
   comment */
class Widget {
    struct Inner { int a; };
};

// a comment continued \
   onto the next line
const char *s = "// not a comment";
const char *raw = R"(a raw string)";
