// mezura-expect lines=21 code=15 comments=3 extra=3 classes=1 structs=1 interfaces=1
using System;

/* a block
   comment */
interface IThing { }

class Thing : IThing {
    struct Point { }
    string a = "// not a comment";
    char quote = '"'; string opener = "/*";
    string b = @"C:\not\escaped";
    string e = @"say ""hi""
// still inside the string
";
    string d = @$"{a}
second line";
    string c = """
a raw block
""";
}
