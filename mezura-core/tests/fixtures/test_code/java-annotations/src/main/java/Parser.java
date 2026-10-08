import com.google.common.annotations.VisibleForTesting;
import org.jetbrains.annotations.TestOnly;

/* A reader with no @Test in its code: one in a comment, one in a string, one in a text block. */
class Parser {
    static final String DOC = "@Test marks a test";
    static final String EXAMPLE = """
            @Test
            void t() {}
            """;

    @VisibleForTesting
    @TestOnly
    public String parse(String text) {
        return text; // @Test
    }

    @Override
    public String toString() {
        return DOC;
    }
}
