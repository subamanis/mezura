// mezura-expect lines=15 code=8 comments=6 extra=1 classes=1 records=1
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
