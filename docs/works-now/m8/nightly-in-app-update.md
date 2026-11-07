- **Updates from inside the app:** a nightly checks once per start whether a newer nightly is out
  (Settings › Agents and updates › Updates › *Check for updates*, on by default). The start panel
  then says *New version available* with **Download update now**, which downloads the package,
  checks its SHA-256, puts it in place of the running build and offers **Restart now**, so a
  tester never has to look for the new build on GitHub. Help › *Check for updates…* does the same
  on request, also with a project open ([spec](docs/spec/updates.md)).
