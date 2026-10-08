// mezura-expect lines=17 code=11 comments=3 extra=3 classes=1 structs=1 interfaces=1
using System;

/* a block
   comment */
interface IThing { }

class Thing : IThing {
    struct Point { }
    string a = "// not a comment";
    string b = @"C:\not\escaped";
    string d = @$"{a}
second line";
    string c = """
a raw block
""";
}
