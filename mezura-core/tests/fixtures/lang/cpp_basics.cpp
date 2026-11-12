// mezura-expect lines=17 code=11 comments=3 extra=3 classes=1 structs=1
#include <string>

/* a block
   comment */
class Widget {
    struct Inner { int a; };
};
enum class Color { Red, Green };

int n = 1;  // trailing
const char *s = "// not a comment";
const char *raw = R"(a raw string
that spans two lines)";
const char *tagged = R"js(a lone " quote
/* not a comment )" still inside
)js";
