# TUI Commands

Commands are entered at the VexCoder prompt inside the terminal UI. All commands begin with `/`.

## Reference

### `/commands` or `/help`

Prints the list of available commands.

### `/clear`

Clears the current conversation and resets the context window. Any active edit loop is cancelled before the conversation is cleared.

### `/history`

Displays the conversation history for the current session.

### `/repo`

Prints the current working directory root that VexCoder is treating as the repository root. This is the boundary enforced for all tool file operations.

### `/ps`

Prints the status of any currently running background processes or pending tool operations.

### `/quit`

Exits VexCoder cleanly. Pending tool operations are cancelled before exit.

## Keyboard shortcuts

Inside the editor, standard readline-compatible shortcuts apply. The editor is a single-line input; multi-line content is pasted as a block.
