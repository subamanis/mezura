import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

@RunWith(JUnit4.class)
public class ParserShapes {
    @Test(expected = IllegalStateException.class)
    public void refuses() {
        new Parser().parse(null);
    }
}
