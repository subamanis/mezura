# mezura-expect lines=15 code=10 comments=4 extra=1 classes=1 modules=1
module Greeter
  class Hello
    NAME = "# not a comment"
    ALT = 'also'
    TEXT = <<~EOS
      it's a heredoc
      # not a comment either
    EOS
  end
end

=begin
a block comment
=end
