- Delay compensation keeps transport and device automation history for the full two-second
  horizon, including one-frame processing blocks. Paths beyond that limit report an error
  instead of silently losing alignment, including nested groups, buses, returns and Main.
