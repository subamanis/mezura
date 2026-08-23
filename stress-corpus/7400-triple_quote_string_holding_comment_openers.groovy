// mezura-real  11 lines 5 code 6 comment
// mezura-count 11 lines 5 code 6 comment
// tokei-real   11 lines 5 code 6 comment
// tokei-count  11 lines 1 code 10 comment
// trap: ''' opens before ' does, so the two comment openers below are text inside the string
// tokei: it reads the single quote first, the string ends with its line, and both openers fire
def a = '''
// this is text inside the string
/* and so is this
'''
def b = 1
