// mezura-expect lines=16 code=7 comments=4 extra=5 structs=1
package main

// a raw string ending in a backslash closes at its own backtick
var sep = `C:\`

/* so this block comment is still a comment */
type Point struct {
	X int
}

var text = `line one
line two holding a " and a /* that open nothing
line three`

// done
