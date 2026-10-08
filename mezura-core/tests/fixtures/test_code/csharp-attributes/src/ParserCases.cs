using System.Collections;
using NUnit.Framework;
using UnityEngine.TestTools;

[TestFixture]
public class ParserCases
{
    [Test, Order(1)]
    public void First() { }

    [UnityTest]
    public IEnumerator Frames() { yield return null; }
}
