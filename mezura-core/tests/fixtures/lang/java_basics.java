// mezura-expect lines=16 code=7 comments=3 classes=2 interfaces=1
package demo;

/* block
   comment */
public interface Greeter {
    String greet();
}

public class Hello implements Greeter {
    public String greet() {
        return "// not a comment";
    }
}

public record Point(int x, int y) {}
