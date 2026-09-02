# mezura-mcp

[![crates.io](https://img.shields.io/crates/v/mezura-mcp.svg)](https://crates.io/crates/mezura-mcp)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/subamanis/mezura#license)

An MCP server that lets a coding assistant run [mezura](https://github.com/subamanis/mezura), the
line counter, on its own: count a directory, get the numbers back as JSON, or ask why one file was
counted the way it was.

## Installation

```bash
cargo install mezura-mcp
```

Or take it from the [latest release](https://github.com/subamanis/mezura/releases/latest), where it
sits beside `mezura` in every archive.

It runs the `mezura` binary and counts nothing itself, so mezura 3 has to be installed too. It looks
for it next to itself first, then on the path. If it lives somewhere else, name it with
`MEZURA_BIN`.

## Setup

Add it to whichever file your editor keeps its servers in:

```json
{
  "mcpServers": {
    "mezura": { "command": "mezura-mcp" }
  }
}
```

With the binary somewhere else:

```json
{
  "mcpServers": {
    "mezura": { "command": "mezura-mcp", "env": { "MEZURA_BIN": "/opt/tools/mezura" } }
  }
}
```

## The tools

- `count_lines_of_code` counts a directory or a file and answers with the report, which is what to
  ask for when a person is going to read the answer.
- `count_lines_of_code_as_json` answers with the JSON document, for when the numbers are going to
  be compared or added up.
- `explain_file` goes through one file line by line and says why each line was counted the way it
  was, which is what to reach for when a number looks wrong. It takes a range of lines, so a long
  file can be asked about without the whole of it coming back.

Every call starts mezura afresh, so nothing of one answer can leak into the next, and a project's
own `.mezura` settings apply exactly as they do on the command line. An answer too long to be read
at once is refused, with a note saying what to ask for in its place.

## License

MIT OR Apache-2.0, at your option.
