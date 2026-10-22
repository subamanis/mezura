# mezura-expect lines=21 code=10 comments=5 extra=6 packages=1
package Greeter;

=head1 NAME

Greeter - says hello, and doesn't mind an apostrophe

=cut

# greets
sub greet {
    my ($name) = @_;
    print "hello $name\n";
    print <<"EOT";
it's a heredoc
# not a comment
EOT
}

my $note = '# not a comment';
1;
