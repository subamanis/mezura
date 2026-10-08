import kotlin.test.BeforeTest

abstract class ParserSetup {
    lateinit var parser: Parser

    @BeforeTest
    fun create() {
        parser = Parser()
    }
}
