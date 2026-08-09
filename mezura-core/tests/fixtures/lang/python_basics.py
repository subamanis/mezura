# mezura-expect lines=17 code=11 comments=3 classes=2
import os

class Foo:
    def __init__(self):
        self.x = 1  # trailing comment

# a full line comment
class Bar(Foo):
    def run(self):
        # comment inside a method
        s = "# not a comment"
        t = 'also # not'
        return s + t

broken = "unbalanced quote
after = 1  # counted right again
