// mezura-expect lines=15 code=9 comments=3 extra=3 classes=1 structs=1 interfaces=1
using System;

/* a block
   comment */
interface IThing { }

class Thing : IThing {
    struct Point { }
    string a = "// not a comment";
    string b = @"C:\not\escaped";
    string c = """
a raw block
""";
}
