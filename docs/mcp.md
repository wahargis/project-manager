# MCP Server

Project Manager ships as a stdio Model Context Protocol server.

```bash
pm --mcp
```

The server exposes the same operations as the CLI as MCP tools. Any MCP-capable agent can read and write project state, query the knowledge graph, create decisions, and inspect next-phase suggestions without leaving its own tool loop.

Configuration notes:

- The server uses the same SQLite database as the CLI.
- The embedded dashboard can auto-start with the server unless the port is busy or `PM_WEB_PORT` is set.
- The server is local-first and does not require network access.
