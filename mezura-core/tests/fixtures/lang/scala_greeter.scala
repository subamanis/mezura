// mezura-expect lines=11 code=7 comments=2 extra=2 classes=1 objects=1 traits=1
/* outer /* inner */ still outer */
val s = "// not a comment"
val raw = """
a raw string
"""

trait Greeter
object Main
class Hello extends Greeter {
}
