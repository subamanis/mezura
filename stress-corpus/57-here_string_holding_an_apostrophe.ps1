# mezura-real  11 lines 4 code 7 comment
# mezura-count 11 lines 4 code 7 comment
# tokei-real   11 lines 4 code 7 comment
# tokei-count  11 lines 4 code 7 comment
# trap: an apostrophe inside a PowerShell here string. The body of a here string is literal and it
# ends only at a closer sitting at the start of a line, so the apostrophe in the word is a byte
$s = @'
it is a here string with an apostrophe: don't
'@
# a real comment
$x = 1
