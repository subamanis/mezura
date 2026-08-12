# mezura-real  12 lines 5 code 6 comment
# mezura-count 12 lines 5 code 6 comment
# tokei-real   12 lines 6 code 6 comment
# tokei-count  12 lines 6 code 6 comment
# trap: a blank line inside a multi line string. A definition and not a mistake: blank to a counter
# that asks what a line says, code to one that asks which region the line sits in
value = """
first line of the string

third line after a blank one
"""
print(value)
