[TestFixture]
public class ParserSuite
{
    public void ReadsANumber() => Assert.AreEqual(1, new Parser().Read("1"));
}
