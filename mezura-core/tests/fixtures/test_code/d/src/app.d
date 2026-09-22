int add(int a, int b) { return a + b; }

unittest {
    assert(add(1, 2) == 3);
}

version(unittest) {
    int helper() { return 1; }
} else {
    int helper() { return 2; }
}

@safe unittest
{
    assert(true);
}

// unittest in a comment
string s = "unittest";
int myunittest = 1;
