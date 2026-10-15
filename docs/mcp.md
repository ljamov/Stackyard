# AI coding tools (MCP)

Let Claude Code, Cursor and other AI tools see and manage your local stack, with you deciding what they may do.

`stackyard mcp` is an [MCP](https://modelcontextprotocol.io) server, so Claude Code, Cursor,
Claude Desktop, Windsurf and other AI tools can see and manage your local stack. Switch on the
**MCP server** module in Settings (or `stackyard modules on mcp`), then add it to your AI tool;
the MCP server screen and `stackyard mcp setup` show the exact line, for example:

```sh
claude mcp add --scope user stackyard -- stackyard mcp
```

AI tools can always look: `list_sites`, `list_services`, `list_runtimes` and `read_logs`. Each
action is off until you allow it on the MCP server screen or with `stackyard mcp allow <tool>`:
`start_service`, `stop_service`, `create_site` and `set_site_runtime`. Deleting sites, databases
or files is never offered. Every call is recorded with the AI tool's name; see it on the MCP
server screen or with `stackyard mcp log`. An AI tool that is already connected sees allowed
tools appear without reconnecting.

---

[← Back to the README](../README.md)
