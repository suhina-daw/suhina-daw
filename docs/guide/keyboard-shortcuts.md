---
description: User guide to the keyboard shortcuts, one entry per action with its default key, where it works and what it does, written for users and for agents
read_when: user guide, keyboard shortcuts, what does this key do, list of keys, every shortcut explained, keybindings, hotkeys, action ids, which action a key runs, back up keys, share keymap, keymap.json, rebind a key, keys for agents
type: howto
status: current
last_verified: 2026-10-16
covers:
  - crates/ui/src/settings/keymap.rs
  - crates/ui/src/settings/settings_view.rs
---

# Keyboard shortcuts

This page lists every keyboard shortcut in Suhina and says what each one does. It is written to be
read by people using Suhina and by agents that need to know which action a key runs.

The technical details (the presets, the mouse switches and the keymap file) are in
[`shortcuts.md`](../shortcuts.md). This page is the plain reference.

## How to read this page

- **Ctrl** means the Ctrl key on Windows and Linux and the **Cmd** key on macOS. **Alt** is
  **Option** on macOS.
- **Key** is the default key. Every key can be changed (see [Changing a key](#changing-a-key)).
- **Action** is the action's id. The id never changes, even when the key does. Settings ›
  Shortcuts and the file `keymap.json` use the id, so it is the name to use when you talk about a
  shortcut, write a guide or tell an agent what to press.
- **Where** says where the key works:
  - **Anywhere**: in any view, as long as you are not typing in a text field.
  - **Piano roll**: only after you have clicked inside the piano roll (the editor that shows a
    MIDI clip's notes). Until you click there, the same keys keep their other uses.
  - **Session slot**: in the Session view, when a clip slot is selected. A slot is one of the boxes
    in a track's column that holds a clip.
  - **Selection**: acts on what is selected. In the arrangement that is the selected clips; in the
    piano roll, after a click in it, the selected notes.

A key does nothing while you type in a text field, such as the browser's search or a track's name.

## File

| Key | Action | Where | What it does |
|---|---|---|---|
| Ctrl+T | `file.new` | Anywhere but the start panel, which offers the same | Opens the project dialog, where you make a new project or open an existing one. The project opens in a tab of its own. |
| Ctrl+O | `file.open` | Anywhere | Opens the same project dialog with the cursor in its file path field, ready to open a project file. The project opens in a tab of its own. |
| Ctrl+S | `file.save` | Anywhere | Saves the project in the file it came from. A project that has never been saved asks for a name and a place first. |
| Ctrl+Shift+S | `file.saveAs` | Anywhere | Saves the project under a new name or in a new place. From then on, Ctrl+S saves into the new file. |
| none | `file.saveTemplate` | Anywhere | Saves the project as a template, so a new project can start from it. |

## Projects (tabs)

Suhina can have several projects open at once, each in a tab.

| Key | Action | Where | What it does |
|---|---|---|---|
| Ctrl+Tab | `projects.next` | Anywhere | Switches to the next project tab. After the last tab it goes back to the first. |
| Ctrl+Shift+Tab | `projects.previous` | Anywhere | Switches to the previous project tab. Before the first tab it goes to the last. |

The project you leave stops playing.

## Playing

| Key | Action | Where | What it does |
|---|---|---|---|
| Space | `transport.toggle` | Anywhere | Starts playback, or stops it when it is playing. Where playback starts and where the playhead goes when it stops are set in Settings › General › **Play from** and **Stop**. |
| Shift+Space | `transport.playFromSelection` | Anywhere | Plays from the start of the clip selected in the Arrangement view, whatever the Play from setting says. With no clip selected it plays from the play start marker. |
| Ctrl+L | `transport.setLoopEnabled` | Anywhere | Turns looping on or off. While looping is on, the arrangement plays the loop region over and over. If there is no loop region yet, turning it on makes one over the first 4 bars. |
| M | `marker.addAtPlayhead` | Anywhere | Adds a marker (a named point in the arrangement) where the playhead is. While stopped, it adds the marker at the play start marker. |

## Views and windows

| Key | Action | Where | What it does |
|---|---|---|---|
| Tab | `view.toggle` | Anywhere | Switches between the Session view (clips in a grid of slots, launched by hand) and the Arrangement view (clips on a timeline). |
| Shift+Tab | `view.detail` | Anywhere | Shows or hides the detail area at the bottom of the window, where the clip editor and the track's effects and instruments are. |
| Ctrl+Shift+E | `view.editor` | Anywhere | Hides the clip editor, or shows it again. In the piano roll, Escape still only clears the note selection. |
| Ctrl+B | `view.browser` | Anywhere | Shows or hides the browser, the panel on the left that lists your samples, presets, effects, instruments and plug-ins. With the Draw-first preset this is Ctrl+Shift+B. |
| Ctrl+F | `browser.search` | Anywhere | Puts the cursor in the browser's search field, and shows the browser when it is hidden. In the field, Up and Down move through the results, Enter adds the selected result to the selected track, and Escape clears the search and then leaves the field. |
| Ctrl+Shift+M | `view.sampleMap` | Anywhere | Shows or hides the sample map in the main area. The sample map places your samples on a map by how they sound, so similar sounds are close together. Escape on the map closes it too. |
| Ctrl+Alt+M | `view.sampleMapWindow` | Anywhere | Opens the sample map in a window of its own, or puts it back in the main window. |
| Ctrl+M | `view.mappingMode` | Anywhere | Starts or stops key mapping. While mapping, you click a clip slot, a scene or a stop button and press a key to launch it with. While the Session view is shown, a key mapped this way wins over its shortcut. |
| Ctrl+, | `settings.open` | Anywhere | Opens Settings, or closes it when it is open. |
| Ctrl+Plus or Ctrl+= | `view.zoomIn` | Anywhere | Makes everything in the window larger, in 10 % steps up to 200 %. Both keys work because on some keyboards Plus needs Shift. |
| Ctrl+Minus | `view.zoomOut` | Anywhere | Makes everything in the window smaller, in 10 % steps down to 50 %. |
| Ctrl+0 | `view.zoomReset` | Anywhere | Puts the window back to its normal size (100 %). |

The zoom level is remembered the next time Suhina starts.

## Editing

| Key | Action | Where | What it does |
|---|---|---|---|
| Ctrl+Z | `edit.undo` | Anywhere | Undoes the last change. Every project has its own undo history. |
| Ctrl+Shift+Z or Ctrl+Y | `edit.redo` | Anywhere | Does again the last change you undid. |
| Ctrl+C | `selection.copy` | Selection | Copies the selected clips, or in the piano roll the selected notes. |
| Ctrl+X | `selection.cut` | Selection | Copies the selected clips or notes and removes them. |
| Ctrl+V | `selection.paste` | Selection | Pastes what you copied. Clips go right after the selected clip, or at the playhead on the selected track. Notes go right after the selected notes, or at the playhead when it is inside the clip. |
| Ctrl+D | `selection.duplicate` | Selection | Puts a copy of the selected clips or notes right after them. With the Draw-first preset this is Ctrl+B. |
| Delete or Backspace | `selection.delete` | Selection | Deletes the selected clips, or in the piano roll the selected notes. |
| 0 | `selection.mute` | Selection | Mutes the selected clips, or unmutes them if they are muted. A muted clip stays in place but makes no sound. |
| Ctrl+A | `selection.selectAll` | Anywhere | Selects every clip in the arrangement. In the piano roll, after a click in it, selects every note instead. |
| Ctrl+E | `selection.split` | Selection | Splits the selected clip in two at the playhead. |
| Ctrl+J | `selection.consolidate` | Selection | Joins the selected clips, and the space between them, into one new clip. |

Every key that acts on clips acts on the whole selection, and all of it is one undo step.

With a time selection in the Arrangement (a drag on empty lanes, on the ruler, or over a clip as
Settings › Arrangement › *Ctrl+drag selects time over clips* says), Delete, Ctrl+X, Ctrl+C,
Ctrl+E, Ctrl+D and Ctrl+J act on what plays inside it: clips crossing its edges are split there.
Ctrl+V puts material copied from several tracks back on its own tracks, from the play start
marker. Ctrl+R edits a clip's name right on the clip; Enter or a click elsewhere keeps it, Escape
leaves it.
Ctrl+click a clip to add it to the selection; Shift+click selects everything from the last
selected clip up to the one you click.

Pasted and duplicated clips replace what they land on, unless the stacking setting for that kind
of clip is on.

## Tracks

| Key | Action | Where | What it does |
|---|---|---|---|
| Ctrl+G | `selection.group` | Selection | Puts the selected tracks in a group track. A group track holds other tracks so you can fold them away and set their volume together. |

## Session view

These keys work in the Session view when a clip slot is selected.

| Key | Action | Where | What it does |
|---|---|---|---|
| Enter | `selection.launch` | Session slot | Launches the clip in the selected slot: it starts playing at the next launch point. |
| Up | `selection.up` | Session slot | Selects the slot above. |
| Down | `selection.down` | Session slot | Selects the slot below. |
| Left | `selection.left` | Session slot | Selects the slot in the track to the left. |
| Right | `selection.right` | Session slot | Selects the slot in the track to the right. |

## Piano roll

The piano roll is the editor that shows a MIDI clip's notes, with pitch from low to high and time
from left to right. Its keys work only after you have clicked inside it. A grid step is the length
of one square of the grid, which follows the zoom and the grid setting.

| Key | Action | Where | What it does |
|---|---|---|---|
| 1 | `pianoRoll.tool1` | Piano roll | Chooses the **Select** tool: click a note to select it, drag to move it or to draw a selection box. |
| 2 | `pianoRoll.tool2` | Piano roll | Chooses the **Draw** tool: click to add a note, drag to set its length. |
| 3 | `pianoRoll.tool3` | Piano roll | Chooses the **Paint** tool: drag to add a note on every grid step you pass over. |
| 4 | `pianoRoll.tool4` | Piano roll | Chooses the **Erase** tool: click or drag over notes to delete them. |
| 5 | `pianoRoll.tool5` | Piano roll | Chooses the **Slice** tool: click a note to cut it in two at that point. |
| 6 | `pianoRoll.tool6` | Piano roll | Chooses the **Mute** tool: click or drag over notes to mute or unmute them. |
| B | `pianoRoll.draw` | Piano roll | Switches between the Select and Draw tools. |
| Ctrl+A | `pianoRoll.selectAll` | Piano roll | Selects every note in the clip. |
| Escape | `pianoRoll.deselect` | Piano roll | Clears the note selection. |
| Left | `pianoRoll.earlier` | Piano roll | Moves the selected notes one grid step earlier. |
| Right | `pianoRoll.later` | Piano roll | Moves the selected notes one grid step later. |
| Up | `pianoRoll.up` | Piano roll | Moves the selected notes one semitone higher. |
| Down | `pianoRoll.down` | Piano roll | Moves the selected notes one semitone lower. |
| Shift+Up | `pianoRoll.octaveUp` | Piano roll | Moves the selected notes one octave (12 semitones) higher. |
| Shift+Down | `pianoRoll.octaveDown` | Piano roll | Moves the selected notes one octave lower. |
| Shift+Left | `pianoRoll.shorter` | Piano roll | Makes the selected notes one grid step shorter. A note never gets shorter than one step. |
| Shift+Right | `pianoRoll.longer` | Piano roll | Makes the selected notes one grid step longer. |
| Ctrl+U | `pianoRoll.quantize` | Anywhere | Quantises the piano roll's notes: moves them onto the grid, with the settings in the quantise block of the MIDI tools column. With notes selected it moves only those, otherwise every note. |

Copy, cut, paste, duplicate and delete in the piano roll use the [Editing](#editing) keys. With no
note selected (or nothing copied, for paste) they act on the clip instead.

The key column on the left of the piano roll plays its notes with the mouse: click or drag on it to
hear the track's instrument. Ctrl+click and Ctrl+drag on it select every note of those pitches.

## Help

| Key | Action | Where | What it does |
|---|---|---|---|
| none | `help.about` | Anywhere | Shows which version of Suhina this is (Help › About Suhina). |

## Commands you can give a key

Besides the actions above, every command that needs no further information can get a key. They
are listed in Settings › Shortcuts under **Commands** and have no key until you give them one. The
same commands run from `suhina-cli` and from agents through MCP ([`mcp.md`](../mcp.md)); the full
list is [`commands.md`](../commands.md).

The ones most useful on a key:

| Action | What it does |
|---|---|
| `transport.play` | Starts playback from the play start marker. Unlike Space, it never stops playback. |
| `transport.stop` | Stops playback, the arrangement and every launched clip at once. Pressing it while stopped moves the play start marker back to the beginning. |
| `transport.followTempoMap` | Makes the arrangement's tempo and time signature play again after a launched scene changed them. |
| `launcher.stopAll` | Stops every launched clip at the next launch point. |
| `launcher.backToArrangement` | Makes every track that launched a clip play its arrangement clips again. |
| `track.add` | Adds a new track. |
| `return.add` | Adds a return track, a track that effects are shared on; other tracks reach it through sends. |
| `mixer.resetClip` | Turns off every clip indicator, the red marks that show a signal was too loud. |
| `preview.stop` | Stops the sound the browser is previewing. |
| `plugins.scan` | Looks for newly installed plug-ins. |
| `library.refresh` | Reads the browser's folders again, to pick up files added outside Suhina. |
| `soundmap.analyse` | Analyses the samples again for the sample map. |
| `project.autosave` | Reports the project's backups and the backup settings. |

`project.save` is left out of this list on purpose: use `file.save` (Ctrl+S).

## Changing a key

1. Open Settings with **Ctrl+,** (Options › Settings) and choose **Shortcuts**.
2. Find the action. Its keys are shown beside it.
3. Click **Change** and press the new key, with its modifiers. Escape cancels.
4. **Clear** removes the action's key. **Reset all** goes back to the defaults of the chosen preset.

A key used by two actions is shown in red at the top of the list.

The **Preset** row at the top of the list offers three starting points: **Select-first** (the
default), **Draw-first** and **Mouse-first**. They differ in the duplicate key and in what the mouse
buttons do in the piano roll; [`shortcuts.md`](../shortcuts.md#presets) has the details.

## Backing up and sharing your keys

Your keys are kept in one file, `keymap.json`:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\Suhina\data` |
| macOS | `~/Library/Application Support/Suhina` |
| Linux | `~/.local/share/suhina` |

- **To back up** your keys, copy `keymap.json` somewhere safe.
- **To move them to another computer** or **share them**, copy the file into the same folder
  there, while Suhina is closed. Suhina reads it when it starts.
- **To go back to the defaults**, use **Reset all** in Settings › Shortcuts, or delete the file
  while Suhina is closed.

Suhina writes every action and its keys into the file, by action id, with the preset and the piano
roll's mouse switches. A short example:

```json
{
  "preset": "Select-first",
  "mouseRightDeletes": false,
  "transport.toggle": ["Space"],
  "edit.redo": ["Ctrl+Shift+Z", "Ctrl+Y"],
  "selection.delete": []
}
```

An action with an empty list has no key. An action the file does not mention keeps its default
key, so a file you write by hand only needs the keys you want to change. Lines Suhina does not understand are skipped, and the status bar says so.

There is no import or export button yet; copying the file is the way to do it for now.
