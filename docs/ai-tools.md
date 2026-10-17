# AI coding tools

The **AI tools** screen installs command-line coding assistants, keeps them up to date and starts
one in a site's folder with one click. Stackyard installs each with npm on its own Node.js, so you
don't need Node.js, npm or admin rights of your own.

| Tool | Maker | Command | npm package | Node.js |
| --- | --- | --- | --- | --- |
| Claude Code | Anthropic | `claude` | `@anthropic-ai/claude-code` | 22+ |
| Codex CLI | OpenAI | `codex` | `@openai/codex` | 18+ |
| Gemini CLI | Google | `gemini` | `@google/gemini-cli` | 20+ |
| GitHub Copilot CLI | GitHub | `copilot` | `@github/copilot` | 22+ |
| Qwen Code | Qwen (Alibaba) | `qwen` | `@qwen-code/qwen-code` | 22+ |
| OpenCode | SST | `opencode` | `@opencode/cli` | 20+ |

Each package is the one its maker's install guide names (the **Guide** link next to each tool
opens that guide). Every tool asks you to sign in the first time it starts; some need a paid plan
(GitHub Copilot CLI needs a Copilot plan).

## Install, update, remove

- **Install** uses the Node.js you already have when it is new enough for the tool (your default
  version first, else the newest). When none is, Stackyard installs Node.js 24 first.
- Stackyard looks up the newest version of each tool on npm when you open the screen, and when you
  click **Check for updates**. A tool with a newer version shows **Update**.
- An update is installed next to the old version and replaces it only once it is complete, so a
  failed update leaves the old version working. Close the tool before updating or removing it: on
  Windows a running tool cannot be replaced.
- Each tool lives in its own folder, `tools/ai/<id>` in Stackyard's data folder (for example
  `C:\stackyard\tools\ai\claude`). It never touches a Node.js version's global packages, and
  removing it deletes that one folder. npm's output from the last install is in
  `logs/ai-tools.log`.

## Open in a site's folder

Pick a site (or **Choose folder…**) under **Open in**, then click **Open** next to a tool. A new
terminal window opens in that folder with the tool running:

- **Windows**: a Command Prompt window (Windows Terminal, when it is your default terminal app).
- **macOS**: a Terminal window. The first time, macOS asks whether Stackyard may control Terminal.
- **Linux**: your desktop's terminal (GNOME Terminal, Konsole, Xfce Terminal, kitty, Alacritty,
  WezTerm, foot, Tilix or xterm).

When the tool ends, the window stays open at a prompt in the folder, with `php`, `node`, `npm`,
`composer` and `mysql` using that project's versions.

## In any terminal

With **Use in your terminal** on (Runtimes screen, needs the Stackyard Shell module), each
installed tool's command (`claude`, `codex`...) works in any new terminal, next to `php` and `node`.
A tool always runs on the Node.js it was installed with, whatever version a project pins, since
these tools need a recent one. If that version was removed, it uses another one that is new
enough.

## Command line

```sh
stackyard ai                          # every tool, its version, and whether an update is out
stackyard ai install claude           # install (or `update claude`)
stackyard ai open claude shop.stack   # a new terminal window in a site's folder (or a folder)
stackyard ai remove claude
```

## Switch it off

The **AI coding tools** module can be switched off in Settings › Modules. The screen disappears and
the tools' commands stop working. Installed tools stay on disk: remove them first, or delete the
`tools/ai` folder.

To let these tools see and manage your sites and services, also switch on the MCP server: see
[mcp.md](mcp.md).

---

[← Back to the README](../README.md)
