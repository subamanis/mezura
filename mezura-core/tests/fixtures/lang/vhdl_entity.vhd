-- mezura-expect lines=12 code=7 comments=4 extra=1
entity counter is
    port (clk : in bit);
end entity counter;

-- an architecture
architecture rtl of counter is
begin
end architecture rtl;
/* a block
   comment */
constant NOTE : string := "-- not a comment";
