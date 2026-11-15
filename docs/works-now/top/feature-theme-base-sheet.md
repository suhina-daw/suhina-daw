- Suhina's own look is a base style sheet under every theme: a theme's rules always win over it, a
  theme that sets only colour variables keeps every state, a rule like `mixer { --text-value: gold; }`
  colours a region through its variables, `"base": "none"` in `theme.json` leaves it out, and a
  `user.css` that sets `--accent` recolours Suhina Studio too. Every built-in theme looks as before, bar a few lit states in the layout showcase themes (Suhina Meadow, Blocks and Sheet), which are rewritten with the layout system.
