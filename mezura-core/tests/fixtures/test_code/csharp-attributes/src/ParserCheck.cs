using NUnit.Framework;

class ParserCheck
{
    [Test]
    public void Parses()
    {
        new Parser().Parse("x");
    }
}
