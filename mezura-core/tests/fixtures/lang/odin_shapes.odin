// mezura-expect lines=9 code=6 comments=2 extra=1 structs=1 enums=1 unions=1
/* outer /* inner */ still outer */
s := "// not a comment"
c := 'x'
raw := `a raw \ string`

Point :: struct { x: int }
Colour :: enum { Red }
Value :: union { int, f32 }
