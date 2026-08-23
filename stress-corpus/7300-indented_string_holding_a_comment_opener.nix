# mezura-real  14 lines 6 code 6 comment
# mezura-count 14 lines 6 code 6 comment
# tokei-real   14 lines 8 code 6 comment
# tokei-count  14 lines 3 code 11 comment
# trap: the '' '' string of Nix, whose body is plain text, holding a # and an unclosed /*
# tokei: both open a comment inside the string, and the block one runs to the end of the file
{ pkgs }:
{
  script = ''
    # this is text and not a comment
    /* and this opens nothing at all
  '';
  keep = 1;
}
