// mezura-expect lines=20 code=11 comments=3 classes=2 interfaces=1
package demo;

/* block
   comment */
public interface Greeter {
    String greet();
}

public class Hello implements Greeter {
    char quote = '"'; String opener = "/*";
    public String greet() {
        return "// not a comment";
    }
}

public record Point(int x, int y) {}
String doc = """
a text block
""";
