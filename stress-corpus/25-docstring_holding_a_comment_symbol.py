# mezura-real  8 lines 5 code 3 comment
# mezura-count 8 lines 5 code 3 comment
# trap: a comment symbol inside a docstring is text, and the docstring itself is a string
def f():
    """
    # this is not a comment, it is inside the docstring
    """
    return 1
