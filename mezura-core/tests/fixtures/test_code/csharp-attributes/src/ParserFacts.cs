using Xunit;

public class ParserFacts
{
    [Fact(Skip = "slow")]
    public void Parses() { }

    [Theory]
    [InlineData("x")]
    public void ParsesEach(string text) { }
}
