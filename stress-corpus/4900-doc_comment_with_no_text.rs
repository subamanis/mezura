// mezura-real  13 lines 1 code 11 comment
// mezura-count 13 lines 1 code 11 comment
// tokei-real   13 lines 1 code 12 comment
// tokei-count  13 lines 1 code 11 comment
// tokei-section Markdown 3 lines 0 code 2 comment
// trap: a doc comment line holding nothing but its marker, which is how a blank line is put
// between two paragraphs of documentation. A bare marker says nothing and is extra to us
// tokei: counted blank, although a blank line inside a '/** */' doc comment of the same language
// counts as comment, so the answer turns on which of the two doc syntaxes was used
/// A documented function.
///
/// The paragraph after the empty doc line.
fn documented() {}
