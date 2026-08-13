// mezura-expect lines=11 code=6 comments=2 extra=3 classes=1 interfaces=1
/* outer /* inner */ still outer */
val s = "// not a comment"
val raw = """
a raw string
"""

interface Greeter

class Hello : Greeter {
}
