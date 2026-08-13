<?php
// mezura-real  15 lines 4 code 9 comment
// mezura-count 15 lines 5 code 8 comment
// tokei-real   15 lines 6 code 9 comment
// tokei-count  15 lines 7 code 8 comment
// real-section HTML 2 lines 1 code 1 comment
// trap: the page switches language at a tag rather than at a file boundary. It switches on its
// mezura: the file is read as php from beginning to end, so the html comment has no symbol and
// tokei: the same, the whole file is php and the html comment is counted as code
?>
<p>hello</p>
<!-- an html comment, which php has no symbol for -->
<?php
echo "hi";
?>
