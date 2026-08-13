// mezura-expect lines=11 code=5 comments=3 extra=3 structs=1
#include <stdio.h>

/* a block comment
   over two lines */
struct Point { int x; };

int main(void) {
    char *s = "// not a comment";
    return 0;  // trailing
}
