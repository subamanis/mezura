// mezura-expect lines=43 code=18 comments=4 extra=21 structs=1 enums=1 macros=1 language=C3
module greeter;

import std::io;

<*
 Greets everybody on the list, one name per line.
 @param [in] names : "the names to greet"
*>
fn void greet(String[] names)
{
	foreach (name : names)
	{
		io::printfn("hello, %s%c", name, BANG); // one name per line
	}
}

struct Greeting
{
	String name;
	Volume volume;
}

enum Volume : int
{
	PLAIN,
	LOUD
}

macro String @render(Greeting g)
{
	return g.volume == Volume.LOUD ? g.name.to_upper(mem) : g.name;
}

const char BANG = '!';

/* A block comment, which C3 lets you /* nest */ inside another one. */
fn void banner()
{
	String help = `run greeter // this is not a comment
	               and this is still the raw string`;
	io::printn(help);
}
