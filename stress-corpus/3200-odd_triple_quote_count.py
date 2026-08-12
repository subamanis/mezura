# mezura-real  11 lines 4 code 7 comment
# mezura-count 11 lines 4 code 7 comment
# tokei-real   11 lines 4 code 7 comment
# tokei-count  11 lines 4 code 7 comment
# trap: a docstring opened on one line and closed on another keeps the lines between out of the
# comment count, since a string is not a comment
a = """one"""
b = """two
still inside the docstring
"""
# a real comment
