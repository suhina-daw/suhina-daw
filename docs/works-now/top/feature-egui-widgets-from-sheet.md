- Themes style egui's own widgets everywhere: a rule for `button` (`:hover`, `:active`,
  `:disabled`, `[checked]`), `text-input` (`:focus`), `menu` or `scroll-bar` reaches the scroll
  bars, text fields, drop-downs, check boxes, menus and pop-ups the views draw outside a style
  scope too, rebuilt only when the styles reset; a theme that leaves them alone looks as before.
