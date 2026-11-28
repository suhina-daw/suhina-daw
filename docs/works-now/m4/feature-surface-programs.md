- Extensions: an extension folder's Lua script runs in Suhina's shipped runner, one process
  per script, watched and restarted; the `extensions.*` commands add, list, restart and remove
  it (an agent runs them over the running app's MCP, and `suhina-cli commands` lists them). A
  running script receives every message of the MIDI ports its manifest claims, system exclusive
  included, and what it sends back reaches the device; its `suhina.command`, `suhina.read` and
  `suhina.subscribe` calls run Suhina's commands on the open project, undo included. The
  Settings › Extensions panel and permission enforcement wait.
