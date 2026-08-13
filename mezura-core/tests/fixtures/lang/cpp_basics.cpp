// mezura-expect lines=13 code=7 comments=3 extra=3 classes=1 structs=1
#include <string>

/* a block
   comment */
class Widget {
    struct Inner { int a; };
};

int n = 1;  // trailing
const char *s = "// not a comment";
const char *raw = R"(a raw string
that spans two lines)";
