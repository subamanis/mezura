// mezura-expect lines=16 code=9 comments=6 extra=1 classes=1 records=1
unit Greeter;

{ a block
  comment }
(* the other pair,
   also a block *)
{ an *) inside stays text }
type
  TName = record
    Value: string;
  end;
  TGreeter = class(TObject)
    procedure Greet;
  end;
const S = '// not a comment';
