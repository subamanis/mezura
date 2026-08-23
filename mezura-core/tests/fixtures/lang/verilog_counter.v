// mezura-expect lines=16 code=9 comments=3 extra=4 modules=1 functions=1 tasks=1
module counter (input clk, output reg [7:0] q);

    /* a block comment
       over two lines */
    always @(posedge clk) q <= q + 1;   // a trailing comment

    function [7:0] doubled(input [7:0] v);
        doubled = v << 1;
    endfunction

    task announce;
        $display("counting // not a comment, /* not one either */");
    endtask

endmodule
