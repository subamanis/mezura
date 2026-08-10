// mezura-real  13 lines 2 code 10 comment
// mezura-count 13 lines 2 code 10 comment
// tokei-real   13 lines 3 code 10 comment
// tokei-count  13 lines 6 code 7 comment
// trap: two character literals on one line, one holding an escaped backslash and one holding a
// quote. Ordinary Rust, and this exact line appears in mezura's own sources
// tokei: every comment under the line is lost, measured on tokei 13.0
fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
// one
// two
// three
