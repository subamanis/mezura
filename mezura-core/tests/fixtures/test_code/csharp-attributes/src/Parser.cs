using System;
using System.ComponentModel;

/* A reader with no attribute of a test framework in its code: one in a comment, one in a string,
   a collection expression over a type named Fact, and an indexer over a field named Test. */
[Serializable]
[Category("Readers")]
class Parser
{
    const string Doc = "[Test] marks a test";
    const string Example = @$"
[Fact]
public void T() {{ }}
";
    static readonly Fact[] Facts = [Fact.Create(1), Fact.Create(2)];
    readonly int[] Test = [1, 2];

    [Obsolete("use Parse")]
    public string Read(string text) => text; // [Test]

    public string Parse(string text) => text;

    public int Lookup(int[] map) => map[Test[0]];
}

class Fact
{
    public static Fact Create(int value) => new Fact();
}
