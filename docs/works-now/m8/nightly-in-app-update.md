- **New nightly available:** a nightly checks once per start whether a newer nightly is out
  (Settings › Agents and updates › Updates › *Check for updates*, on by default) and says so in a
  line on the start panel or, with a project open, a bar across the top of the workspace, not a
  pop-up. **Download update now** downloads the package, checks its SHA-256, puts it in place of
  the running build and offers **Restart now**; **Later** hides the offer until a newer build is
  out. A tester never has to look for the new build on GitHub
  ([spec](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/updates.md)).
