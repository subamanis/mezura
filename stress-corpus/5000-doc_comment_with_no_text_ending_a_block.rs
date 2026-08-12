// mezura-real  12 lines 1 code 11 comment
// mezura-count 12 lines 1 code 11 comment
// tokei-real   12 lines 1 code 11 comment
// tokei-count  11 lines 1 code 10 comment
// tokei-section Markdown 1 lines 0 code 1 comment
// trap: the same empty doc comment line, this time with no further documentation under it. What
// makes it its own case is that the answer stops being about comments and becomes about lines
// tokei: the '///' line is counted as neither comment nor blank, so a file of 12 lines is reported
// as 11. The doc body becomes a Markdown section, which is where that classification comes from
/// A documented function.
///
fn documented() {}
