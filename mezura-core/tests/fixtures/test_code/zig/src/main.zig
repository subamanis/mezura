const std = @import("std");

fn add(a: u8, b: u8) u8 {
    return a + b;
}

test "add" {
    try std.testing.expect(add(1, 2) == 3);
}

test {
    _ = add;
}

// test in a comment
const label = "test";
fn test_helper() void {}
