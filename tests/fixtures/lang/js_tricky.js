// mezura-expect lines=12 code=8 comments=1 classes=1
const a = "/* not a comment */";
const b = 'http://example.com';
/* inline */ const c = 1;
const d = 2; /* trailing block
   still comment */ const e = 3;

class Widget {
    render() {
        return a + b + c + d + e;
    }
}
