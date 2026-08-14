# mezura-real  10 lines 1 code 8 comment
# mezura-count 10 lines 1 code 8 comment
# tokei-real   10 lines 1 code 9 comment
# tokei-count  10 lines 3 code 7 comment
# trap: a counted bracket comment is closed only by an end carrying the same count
# tokei: the comment is ended at the bare ]], so the text beside it and the real end count as code
#[==[ a bracket comment
]] not the end
]==]
set(X 1)
