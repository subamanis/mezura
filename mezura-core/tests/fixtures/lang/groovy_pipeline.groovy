// mezura-expect lines=18 code=9 comments=3 extra=6 classes=1 interfaces=1 traits=1
interface Greeter {
    String greet()
}

trait Loud {
    String shout() { 'LOUD' }
}

/* a block comment
   over two lines */
class Hello implements Greeter, Loud {
    String greet() {
        def banner = """a // here and a /* here
open nothing at all"""
        return banner + 'a " inside single quotes'   // a trailing comment
    }
}
