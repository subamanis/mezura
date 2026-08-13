-- mezura-expect lines=8 code=5 comments=2 extra=1 packages=1 types=1
package Shapes is
   type Colour is (Red, Green);
   Name : String := "-- not a comment";
end Shapes;

-- a full line comment
X : Integer := 1;  -- trailing
