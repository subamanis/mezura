import org.junit.jupiter.api.BeforeEach;

abstract class ParserLifecycle {
    Parser parser;

    @BeforeEach
    void create() {
        parser = new Parser();
    }
}
