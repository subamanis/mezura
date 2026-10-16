# mezura-expect lines=23 code=10 comments=5 extra=8 classes=1
###
A block comment over three lines,
holding a # that is text.
###
class Greeter
  constructor: (@name) ->

  greet: -> "Hello ### not a comment #{@name}"  # a trailing comment

### a closed block ### answer = 42
###########
# A banner is a line comment, whatever its length
###########
x = '# not a comment either'
config = {
  debug: true
}
html = """
  <p>### not a comment</p>
  # nor this"""
###* documentation
###
