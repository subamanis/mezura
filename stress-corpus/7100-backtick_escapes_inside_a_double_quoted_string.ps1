# mezura-real  9 lines 2 code 7 comment
# mezura-count 9 lines 2 code 7 comment
# tokei-real   9 lines 2 code 7 comment
# tokei-count  9 lines 3 code 6 comment
# trap: the backtick escapes inside PowerShell's double quoted string, so the quote in the middle
# tokei: the backtick is no escape to it, so the last quote opens a string that crosses lines
Write-Host "a `" b"
# a real comment
$x = 1
