using NUnit.Framework;

public abstract class ParserFixture
{
    protected Parser parser;

    [SetUp]
    public void Create() => parser = new Parser();
}
