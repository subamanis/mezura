using System;
using System.Collections.Generic;
using System.ComponentModel;

/* A reader with no test attribute in its code, only lookalikes in a comment, a string, a
   collection expression, indexers and a dictionary key. */
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

    public int Pick(int[][] grid) => grid[0][Test[1]];

    Dictionary<int[], string> Names => new() { [Test] = "first" };
}

class Fact
{
    public static Fact Create(int value) => new Fact();
}
