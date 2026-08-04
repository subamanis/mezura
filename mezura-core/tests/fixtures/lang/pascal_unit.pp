// mezura-expect lines=12 code=8 comments=3 extra=1 classes=1 records=1
unit Greeter;

{ a block
  comment }
type
  TName = record
    Value: string;
  end;
  TGreeter = class(TObject)
    procedure Greet;
  end;
