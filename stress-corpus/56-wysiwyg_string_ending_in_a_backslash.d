// mezura-real  11 lines 2 code 9 comment
// mezura-count 11 lines 2 code 9 comment
// tokei-real   11 lines 2 code 9 comment
// tokei-count  11 lines 3 code 8 comment
// trap: a wysiwyg string whose last byte is a backslash. In D the body of r"..." is literal, so
// the closing quote closes it whatever precedes it, and the line ends where it looks like it ends
// tokei: the backslash cancels the closing quote, so the string stays open and the comment under
// it is counted as code
auto s = r"C:\temp\";
// a real comment
int x = 1;
