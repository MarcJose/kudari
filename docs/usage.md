# Using Kudari

F12 brings Kudari down and sends it away again, and starting `kudari` a second time does
the same. The settings are behind the menu button in the bar at the bottom. `man kudari`
describes the command line, the D-Bus interface and the files.

## Coming from Yakuake

The first start offers to take over window settings and keyboard shortcuts from
Yakuake, and font, colors, history and cursor from Konsole's default profile. These files
are only read. The import is also available later under Settings → Behavior. If Yakuake
uses the same global shortcut, Kudari asks before taking it over.

## Customizing

- There is a single bar at the bottom: Plasma's own tabs, which show the title of their
  terminal and scroll when they do not fit, and the buttons of the window, with icons from
  the icon theme. It looks like the rest of the desktop and follows it when it changes. The
  slim grip below it changes the height.
- The terminal takes its colors from the Plasma theme, so there are no color schemes to
  maintain. Colors that would be hard to read on its background get closer to the text color.
  Under Settings → Appearance, you can choose a fixed Plasma theme for the terminal instead.
- The command and start directory of new terminals, the title format, mouse behavior
  and the terminal bell are under Settings → Terminal Behavior. The command understands
  quotes and backslashes but no shell syntax. Use `sh -c '...'` for that. If it cannot
  start, the terminal says so and opens the login shell.
- Margins, line numbers, a vertical line, the badge, cursor and tab colors are under
  Settings → Terminal. A single tab gets its own color from its context menu.
- Konsole's `.keytab` files go into `~/.local/share/kudari/keytabs/`, and you choose one
  under Settings → Terminal Behavior. They only change the keys they name.
- The context menu of a terminal adjusts its scrollback and mouse tracking for that
  terminal alone. "One-Shot Monitors" in the tab menu ends a monitor after its first alert.
- A double click on the scrollbar sets or removes a marker. Search matches appear there
  while "Highlight All Matches" is on.
- File links accept `name:line:column`. With an editor command such as
  `kate --line LINE --column COLUMN PATH`, they open at that position.

## Security

- The D-Bus methods `runCommand` and `runCommandInTerminal` are off by default,
  because any program of the same user could run commands through them. Enable
  them under Settings → Security if a script needs them.
- Pasted text loses its control characters, so it cannot end a bracketed paste or
  inject terminal sequences. Pasting line breaks into a program without bracketed
  paste asks for confirmation.
- Programs can write to the clipboard through OSC 52, but not read it.
- Links open on Ctrl+click or from the context menu, and only for `http`, `https`
  and `mailto`, unless you change the list under Settings → Terminal Behavior. Paths to
  existing files work the same way, except executables and `.desktop` files, which are
  never started.

## AI agents

An AI agent such as Claude Code can read the terminals and, if you allow it, work in them.
Its client starts `kudari mcp`, which offers the running Kudari over the
[Model Context Protocol](https://modelcontextprotocol.io). For Claude Code:

```sh
claude mcp add --scope user kudari -- kudari mcp
```

Other clients take the command `kudari` with the argument `mcp`, for example as
`{"mcpServers": {"kudari": {"command": "kudari", "args": ["mcp"]}}}`.

The agent can list the sessions and terminals, read a terminal, type into it, press keys
such as Ctrl+C, and open and close sessions of its own. Settings → Security → AI Agents
decides what it may do:

- **Allow AI agents to read terminals** is off. Switched on, an agent sees everything the
  terminals show, including passwords or tokens that appeared there.
- **Allow AI agents to control terminals** is off. Switched on, an agent can type, press
  keys, and open and close sessions. It can close only the sessions it opened, and never
  types into a terminal whose keyboard input you locked.
- **Ask before running their commands** is on. An agent then types only at the shell
  prompt, not into a running program, and never presses Return: you see each command in
  the terminal and run it yourself. "At the prompt" means that the line editor of the shell
  is active, which bash, zsh and fish show with bracketed paste. A script that asks a
  question gets nothing from an agent, and shells without bracketed paste, such as dash,
  need this switched off. Escape is not available either, since together with the next text
  it could trigger a key binding of the shell that runs the line. Ctrl+C to stop a program always works. Switched off,
  an agent runs commands and answers prompts of programs on its own.

Once switched on, Kudari still asks you before each new agent gets access. The question
names the program that started `kudari mcp`, for example `claude`. Allow it only if you
started that agent yourself. The answer holds until the agent ends, so an agent that starts
anew asks again, and so does every other program. While an agent has access, an eye button
appears in the bar, and a click on it ends the access. Changing the settings for
agents ends every access as well.

`kudari mcp` has no rights of its own: it asks the running Kudari over D-Bus, through the
interface `io.github.marcjose.kudari.Agent` on `/kudari/agent`, which follows the same
settings. Like the rest of the D-Bus interface, it is open to every program of your user,
which is why Kudari asks before each agent. Programs choose their own names, so the name
in the question is a hint, not proof.

## D-Bus

The service `io.github.marcjose.kudari` offers the interface `io.github.marcjose.kudari` on
`/kudari/window`, `/kudari/tabs` and `/kudari/sessions`. Method names match
Yakuake's, so scripts only need the new names. For example, this opens a new session:

```sh
qdbus6 io.github.marcjose.kudari /kudari/sessions addSession
```
