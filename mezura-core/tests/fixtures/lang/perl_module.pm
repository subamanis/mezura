# mezura-expect lines=10 code=5 comments=2 extra=3 packages=1
package Greeter;

# greets
sub greet {
    my ($name) = @_;
    print "hello $name\n";
}

1;
