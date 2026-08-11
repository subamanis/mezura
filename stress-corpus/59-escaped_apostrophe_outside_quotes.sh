# mezura-real  13 lines 2 code 11 comment
# mezura-count 13 lines 4 code 9 comment
# tokei-real   13 lines 2 code 11 comment
# tokei-count  13 lines 2 code 11 comment
# trap: a backslash before an apostrophe in unquoted shell text, which is how one writes an
# apostrophe without quoting it
# mezura: the single quote is declared as a form that escapes nothing, and that answer is applied
# to the opener as well as to the closer, so the backslash is a byte and the apostrophe opens a
# string that runs to the end of the file
echo I\'m done
# a real comment
# a second real comment
x=1
