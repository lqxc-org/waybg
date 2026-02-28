const std = @import("std");

pub const ParseVersionError = error{
    MissingVersion,
    InvalidVersionFormat,
};

pub fn readPackageVersion(allocator: std.mem.Allocator, zon_path: []const u8) ![]const u8 {
    const zon_content = try std.fs.cwd().readFileAlloc(allocator, zon_path, 1024 * 1024);
    defer allocator.free(zon_content);

    var line_it = std.mem.tokenizeScalar(u8, zon_content, '\n');
    while (line_it.next()) |line| {
        const trimmed = std.mem.trim(u8, line, " \t\r");
        if (!std.mem.startsWith(u8, trimmed, ".version")) {
            continue;
        }

        const first_quote = std.mem.indexOfScalar(u8, trimmed, '"') orelse return ParseVersionError.InvalidVersionFormat;
        const after_first_quote = trimmed[first_quote + 1 ..];
        const second_quote = std.mem.indexOfScalar(u8, after_first_quote, '"') orelse return ParseVersionError.InvalidVersionFormat;

        return try allocator.dupe(u8, after_first_quote[0..second_quote]);
    }

    return ParseVersionError.MissingVersion;
}
