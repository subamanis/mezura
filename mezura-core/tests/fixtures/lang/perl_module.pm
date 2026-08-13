# mezura-expect lines=11 code=6 comments=2 extra=3 packages=1
package Greeter;

# greets
sub greet {
    my ($name) = @_;
    print "hello $name\n";
}

my $note = '# not a comment';
1;
