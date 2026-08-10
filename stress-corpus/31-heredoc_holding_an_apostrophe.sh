# mezura-real  10 lines 3 code 7 comment
# mezura-count 10 lines 5 code 5 comment
# trap: an apostrophe inside a heredoc body
# mezura: the apostrophe opens a string and every comment under it is counted as code. A heredoc
# names its own closer at runtime, which no declaration of symbols can express
cat <<TEXT
it's fine in here
TEXT
# a real comment
# a second real comment
