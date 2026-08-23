// mezura-expect lines=16 code=6 comments=4 extra=6 contracts=1 functions=1 structs=1
pragma solidity ^0.8.0;

/// a doc comment
contract Vault {

    struct Deposit {
        uint256 amount;
    }

    /* a block comment
       over two lines */
    function label() public pure returns (string memory) {
        return "a // and a /* open nothing";   // a trailing comment
    }
}
