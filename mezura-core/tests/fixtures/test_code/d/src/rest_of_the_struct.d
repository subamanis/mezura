struct S {
    int a;
    version(unittest):
    int probe;
    int b;
}

int after() { return 0; }
