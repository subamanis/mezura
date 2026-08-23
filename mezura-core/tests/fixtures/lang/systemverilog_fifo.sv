// mezura-expect lines=14 code=9 comments=3 extra=2 modules=1 classes=1 interfaces=1
interface bus_if;
    logic ready;
endinterface

module fifo (bus_if bus);   // a trailing comment
    /* a block comment
       over two lines */
    assign bus.ready = 1'b1;
endmodule

class Packet;
    string label = "holds // and /* which open nothing";
endclass
