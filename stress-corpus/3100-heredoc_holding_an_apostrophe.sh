# mezura-real  13 lines 3 code 10 comment
# mezura-count 13 lines 5 code 8 comment
# tokei-real   13 lines 3 code 10 comment
# tokei-count  13 lines 5 code 8 comment
# trap: an apostrophe inside a heredoc body
# mezura: the apostrophe opens a string and every comment under it is counted as code. A heredoc
# names its own closer at runtime, which no declaration of symbols can express
# tokei: the same, the apostrophe opens a string and the comments under it are counted as code
cat <<TEXT
it's fine in here
TEXT
# a real comment
# a second real comment
