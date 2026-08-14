// mezura-real  13 lines 2 code 10 comment
// mezura-count 13 lines 2 code 10 comment
// tokei-real   13 lines 2 code 11 comment
// tokei-count  13 lines 4 code 9 comment
// trap: Odin block comments nest, so the first closer ends only the inner one and the text under
// it is still inside the outer block
// tokei: the block is ended at the inner closer, so the two lines after it are counted as code
package main
/* outer
   /* inner */
   still a comment
*/
x := 1
