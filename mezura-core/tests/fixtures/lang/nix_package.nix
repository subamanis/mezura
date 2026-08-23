# mezura-expect lines=15 code=9 comments=3 extra=3
{ pkgs }:

/* a block comment
   over two lines */
pkgs.stdenv.mkDerivation {
  pname = "demo";   # a trailing comment
  buildPhase = ''
    echo "a # here opens nothing"
    make all
  '';
  meta = {
    description = "holds a # and a /* that open nothing";
  };
}
