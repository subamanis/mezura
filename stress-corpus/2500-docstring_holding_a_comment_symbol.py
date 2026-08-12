# mezura-real  10 lines 5 code 5 comment
# mezura-count 10 lines 5 code 5 comment
# tokei-real   10 lines 5 code 5 comment
# tokei-count  10 lines 5 code 5 comment
# trap: a comment symbol inside a docstring is text, and the docstring itself is a string
def f():
    """
    # this is not a comment, it is inside the docstring
    """
    return 1
