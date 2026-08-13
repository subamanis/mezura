// mezura-expect lines=12 code=8 comments=1 extra=3 classes=1 interfaces=1
class Greeter {
  fn Greet[self: Self]() -> i32;
}

interface Named {
  fn Name[self: Self]() -> i32;
}
var s: String = "// not a comment";
var t: String = '''
a raw block
''';
