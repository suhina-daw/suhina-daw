- With render threads on (Settings › Audio › Advanced › Render threads), chains with plug-ins run
  on the render workers too: each worker waits for the plug-ins of the chains it holds together,
  so their round trips overlap, every live plug-in keeps its one-block deadline, and an offline
  render is the same on any number of threads, bit for bit. The branches of a device group send
  their plug-ins the block at the same time, so a group takes as long as its slowest branch.
- Each plug-in's own processing time comes back with its block's answer: the device panel's late
  line shows it ("49% of blocks late · plug-in up to 31 ms"), the trace's `audio:` line gives the
  longest plug-in time beside the longest round trip, and `device.plugin` gives
  `longest_process_us` and `average_process_us`, so a slow plug-in and a host that woke late can
  be told apart.
