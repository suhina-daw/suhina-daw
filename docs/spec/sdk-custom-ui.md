---
description: Self-contained device canvases, image assets, drawing expressions and programmable parameter controls
read_when: custom SDK control, device background, icon, photo, texture, XY pad, drawing API, image asset
type: spec
status: current
last_verified: 2026-10-21
covers:
  - crates/device-sdk/src/builtin.rs
  - crates/device-registry/src/lib.rs
  - crates/session/src/rack.rs
  - crates/ui/src/app.rs
  - crates/device-sdk/src/ui/author.rs
  - crates/device-sdk/src/ui/widgets.rs
  - crates/device-sdk/src/ui.rs
  - crates/ui/src/detail/device_canvas.rs
  - crates/ui/src/detail/detail_view.rs
  - crates/device-sdk/examples/custom_panel.rs
  - crates/device-sdk/examples/orbit_filter.rs
---

# Custom device panels

## Compact and expanded panels

Every device has a compact panel in the device chain, at most 168 px high (the detail area's
default 200 px less the 20 px header and 6 px padding above and below). Its width is the
author's, 260 px by default. A device without a canvas shows its generated parameter list,
in columns 260 px wide and up to 168 px tall: a longer list continues in another column to the
right, and the panel widens rather than scrolls. Stock controls a canvas leaves shown sit in the
same columns to the canvas's right. In a row a device is as wide as its canvas and columns plus
12 px (220 px at least, 272 px for one column); the stacked column is 272 px wide by default. A
smaller detail area or column scrolls the panel.

A device may also have an expanded panel: one larger fixed-size canvas (`device.expanded` in a
`.sd` file, `expanded` in a built-in's layout), with the same fields and limits as the compact one.
A device with one has a button beside the X in its header, labelled **Expand** *name* (and
**Close** *name*'s **expanded panel** while it is open). The panel opens over the main area with
the device's name and a close button (**Close expanded panel**) along its top; its canvas is
centred, and scrolls when the main area is smaller. The close button, Escape or a second click on
the header button closes it, and it closes by itself when its device is no longer shown (another
track selected, the device removed). Its controls send the same `device.setParam` as the compact
panel's, so undo, automation and presets do not depend on which is open. No zoom: canvases
scroll and never scale.

## Layouts for built-in devices

A built-in effect, instrument or MIDI effect has no `.sd` file. Its panel comes from
`suhina_device_registry::panel(id)`: its parameters from the device's `DeviceInfo`, in order,
and its layout (`suhina_device_sdk::builtin::Layout`) when it has one. A layout holds only:

- `ui`: the compact canvas (`null` keeps the generated list in the chain);
- `expanded`: the expanded canvas;
- `parameters`: by parameter id, how each is presented: `control` and the `.sd` parameter `ui`
  fields. A choice keeps the device's labels unless the layout gives its own.

It never declares a parameter: name, range, unit, scale and default are the device's. A layout
is refused when it names a parameter its device lacks, gives one a control its range does not allow
(a logarithmic slider below zero, a toggle outside 0..1), or draws a canvas the rules here refuse;
the registry then shows the generated list. The registry's tests fail on such a layout, so a
shipped one cannot drift from its device. A device crate gives its layout in Rust (the Sampler's
`layout()`) or as `devices/<name>/panel.json`, listed in `suhina_device_registry::LAYOUT_FILES`:

```json
{
  "ui": { "size": [260, 168], "controls": [ ... ] },
  "parameters": { "gain": { "control": "knob", "ui": { "decimals": 1 } } }
}
```

The generated list gives a continuous parameter a slider (logarithmic on a logarithmic scale), a
toggle a check box, a whole number a stepped slider and a choice a menu of its labels. A plug-in
gets the same list from the parameters its host lists, under its editor button while its editor is
not inside the panel: hidden and read-only parameters are left out, a switch is a check box and a
stepped parameter moves in its steps; the value shows as the plug-in writes it until it is changed
here. The list is read again every two seconds while the panel shows, in the background, so a
change in the plug-in's own editor reaches it.

## Rust control constructors

`CustomControl` provides `number`, `dial`, `toggle`, `set_button`, `menu`, `value_menu`,
`label` and `separator` constructors. They construct the existing serialized controls, with
standard defaults for gestures, bindings and drawings; the host interaction and theme paths
are unchanged. `widget` is the lower-level passive-widget constructor for custom composition.
Number/dial controls bind the vertical axis; toggle, set-button and menu controls bind X.
Number/dial defaults include the standard precision/reset tooltip and a default accessible
name, which authors should override when the parameter id is not a meaningful label.

Configure a control before inserting it; no operation implicitly edits the panel's last control:

```rust
let control = CustomControl::number("time_left", [64., 48., 50., 24.])
    .name("Left delay time")
    .logarithmic(true)
    .visible_when(UiValue::parameter("sync_left", false).equals(0.));
ui.controls.push(control);
```

Chainable `bounds`, `id`, `name`, `tooltip`, `visible_when` and `logarithmic` setters retain ordinary
`CustomControl` fields for custom X/Y bindings. Fallible `format`, `decimals`, `prefix` and
`off_label` setters configure supported widgets and return an error on an incompatible widget;
precision must be within 0..8. For example, `.format(TextFormat::Number)?.decimals(2)?`
overrides inherited formatting. `UiBinding` also provides `logarithmic`.
`UiValue::equals` constructs the existing bounded equality expression.

`parameter_metadata(&parameter)` reuses the name and the declared linear/logarithmic control
mapping for matching bindings, leaving other parameter bindings unchanged. Numeric widgets also
inherit `parameter.ui.decimals`; Choice/Radio controls use choice text, a declared `Hz` unit
selects frequency text, and other declarations use number formatting. This never infers the
signal scale from the unit.
Explicit setters applied afterwards override these defaults. Units and choice
labels continue to resolve from parameter metadata in the host. It never guesses a scale from
the id/unit, changes a control id, hides stock parameter rows, or edits a parameter declaration.
Several controls may bind one parameter, but must have distinct control ids; use `.id(...)`
when the default parameter-based identity would collide. Label/separator ids are supplied explicitly.
Set-buttons default to `<parameter>_<value>`; toggles, numbers, dials and menus default to the
parameter id. Bounds, expressions, parameter references and widget contracts still pass through
the existing device UI validator when the graph is saved or compiled. Keyboard operation,
accessibility and gesture-based command/undo behavior are shared with manually built controls.

### Groups with local coordinates

`ControlGroup::new([x, y])?` defines an authoring-only origin. Pass controls with local bounds
through `group.place(control)?` before inserting them. Changing that origin and rebuilding moves
all controls placed through the group, including its captions; local drawing coordinates,
bindings, ids and insertion order remain unchanged. Origins and local bounds must be finite and
nonnegative, with controls at least 8 pixels wide/high and final bounds within 8192 pixels.
The normal graph validator also checks placement against the actual canvas size.

```rust
let tone = ControlGroup::new([384., 0.])?;
ui.controls.push(tone.place(
    CustomControl::number("cutoff", [60., 36., 56., 18.])
        .parameter_metadata(cutoff_parameter)
        .decimals(0)?,
)?);
```

Placement flattens to existing absolute `.sd` bounds. There is no serialized group, runtime
layout engine, automatic resizing or extra renderer path. Delay uses shared origins for TIME,
FEEDBACK, MIX, TONE and MOTION; compatibility tests keep the complete saved panel unchanged.

The `custom_panel` example writes `custom-panel.sd`, whose `device.name` is **Velvet Volume**.
It demonstrates that the file name and the inserted device's name may differ, using a standard
gain control and an explanation inside the panel.

The separate `orbit_filter` example writes `orbit-filter.sd`, named **Orbit Filter**. It demonstrates a
self-contained custom UI: a gradient background, embedded raster texture, a drawn XY pad that
controls cutoff and stereo balance, and a drawn output knob. It hides the generated parameter
controls. Instructions appear inside the authored panel.

Generate the two files with:

```sh
cargo run -p suhina-device-sdk --example custom_panel -- target/sdk-examples/custom-panel.sd
cargo run -p suhina-device-sdk --example orbit_filter -- target/sdk-examples/orbit-filter.sd
```

`device.ui` optionally describes a canvas. It contains size, background RGBA, embedded image
assets, drawing layers, custom controls and a list of parameters hidden from the stock panel.
Everything is stored in the single JSON `.sd` and then embedded in the project. Old files
without a canvas use their existing controls. Built-in SDK widgets and stock parameter controls
read the active Suhina theme at render time; authored drawing colours remain explicit. No external
paths or executable UI code are read from a device file. Rust authors program panels by constructing the SDK's drawing expressions
and input bindings. Normalized expressions can select logarithmic mapping explicitly. Standard controls remain available beside the canvas.

Canvas `size: [width, height]` defaults to `[640, 240]`. Optional
`viewport_size: [width, height]` sets the preferred scrollable device body independently of the
canvas, including node rows and stock controls. Both dimensions use logical pixels in 24..8192.
The host adds a 20-pixel header and 6-pixel padding on each side. Available host space clips the
viewport; oversized contents scroll without scaling. This works in row and stacked layouts.
Omitted or null `viewport_size` preserves automatic sizing: row width is at least 220 or canvas
width plus 12, row height fills the detail area; stacked width fills the column (normally 272),
and height fits node rows, canvas and stock controls. Setting `size` does not scale artwork.

Canvas size is 24..8192 logical pixels per axis. Up to 128 custom controls and 1024 total drawing
commands are accepted. Each control has a unique id, accessible name, tooltip, bounds, drawings,
a gesture and optional X/Y parameter bindings. Bounds stay inside the canvas. Drawing positions
are local to the canvas or the control. Layer order is array order; controls draw after layers.

Drawing commands: rectangle, circle, line, path, arc, text, image and four-corner gradient.
Paths have at most 256 points; filled paths must be convex. Arc tessellation is bounded. Colours are RGBA byte quadruples.
Text can append a parameter's current value with bounded decimal precision. Numeric drawing
values can be literals or bounded expressions (parameter, normalized parameter, add, multiply,
sine, cosine, exponential, natural logarithm, equality and oscillator waveforms). Expression depth is at most 16, with 32768 expression nodes per canvas;
non-finite results become zero, finite values are bounded before drawing. This supports moving
markers, response drawings, circular handles, segmented displays and meters driven by parameters.
No DSP outputs or audio-thread buffers are read by the UI.

Image assets use stable ids and base64 PNG/JPEG bytes. File size is at most 2 MiB; compressed image
bytes before base64 total at most 1 MiB, one asset at most 512 KiB, decoded dimensions at most 4096 per axis,
one image at most 1 million pixels and total images at most 4 million pixels. Header dimensions
are validated before decoding; image decoding uses allocation limits off the audio thread.
Decode failures appear in the panel as an asset error and preserve the graph. Textures and parsed
UI graphs are cached with bounded retention. Supported image layouts are stretch, contain and
cover, with UV crop, RGBA tint and optional tile size. Repetition uses texture wrapping, not an
unbounded loop of drawing operations. Vector icons are paths/lines/circles, raster icons can
use PNG transparency. Photos and patterns are original assets supplied by the author.

Gestures: drag (relative X/Y), position (absolute pointer position, Y upward), toggle, momentary.
Bindings map normalized coordinates to parameter units with optional logarithmic mapping.
Keyboard arrows edit axes, Shift provides fine adjustment, double click resets defaults;
Space/Enter activate buttons. A gesture `{"set": 3}` sets the X parameter to the literal integer 3; its value must be inside the declared range. The control name and values are exposed to accessibility.
A `{"tab": 2}` gesture selects value 2, or returns to zero when already selected.
Tabs bind only X to a linear parameter whose minimum is zero; the positive tab
value must be in range. Button widgets show exact-value selection for tabs.
Click and Space/Enter share the same command and undo path.
Edits dispatch `device.setParam` with a shared gesture id so a two-axis drag is one undo gesture.
All bindings reference declared parameter ids, and values use their range/discrete clamping.

A canvas may hide stock parameter rows with `hidden_parameters`; this only changes presentation.
All hidden values remain available through commands and project state. UI resources do not run
inside the DSP callback. UI code never loads a user native library or script.

## Serialized drawing fields

All drawings have a `kind`. Rectangles are `[x,y,width,height]`, points are `[x,y]`; geometry
accepts `UiValue` numbers or expression objects. Colours are `[r,g,b,a]` in 0..255.

| kind | Required fields | Optional fields |
| --- | --- | --- |
| `rect` | `rect`, `color` | `radius` (0) |
| `circle` | `center`, `radius`, `color` | |
| `line` | `from`, `to`, `color` | `width` (1) |
| `path` | `points`, `color` | `width` (1), `closed` (false), `fill` (none, convex) |
| `arc` | `center`, `radius`, `start`, `end`, `color` | `width` (1); angles in radians |
| `text` | `position`, `text`, `color`, `size` | `parameter` (none), `decimals` (0) |
| `image` | `asset`, `rect` | `uv` ([0,0,1,1]), `tint` (white), `tile_size` (none), `fit` (`stretch`) |
| `gradient` | `rect`, `colors` | Colours ordered top-left, top-right, bottom-right, bottom-left |

Stroke widths and rectangle corner radii are 0..128; text size is 8..48. Image `fit` also accepts
`contain` and `cover`. `tile_size` overrides fit with repetition. `text.parameter` appends the
current value and unit to the prefix `text`. `decimals` is 0..8. Unknown fields are rejected.

A value expression uses `op`: `parameter` has `parameter` (id), `normalized` (false), `logarithmic`
(false); `add` and `multiply` have `a` and `b`; `sin` and `cos` have `value` (radians).
The `equal` expression has `a` and `b` and returns 1 when their difference is less than 0.0001, otherwise 0; use it for selected segment highlights.
The parameter expression returns raw units unless `normalized` is true. Use the same
`logarithmic` flag on bindings and normalized drawings.

A serialized control referencing a declared continuous `cutoff` parameter:

```json
{
  "id":"frequency", "name":"Cutoff", "tooltip":"Drag vertically",
  "bounds":[16,16,100,100], "gesture":"drag",
  "y":{"parameter":"cutoff","logarithmic":true},
  "drawing":[
    {"kind":"rect","rect":[0,0,100,100],"radius":0,"color":[37,36,33,255]},
    {"kind":"text","position":[8,40],"text":"","parameter":"cutoff",
     "decimals":0,"color":[166,163,255,255],"size":14}
  ]
}
```

`x`/`y` bindings default to none; `tooltip` and `drawing` default to empty; `gesture` defaults to
`drag`. `id`, `name`, and `bounds` are required. A canvas defaults to 640x240 with background
[37,36,33,255], empty assets/layers/controls/hidden_parameters. Asset objects require `id` and
`data` (standard base64). Runtime controls bind only declared parameters, never paths or commands.

## Conditional controls and text layout

A custom control can set `visible_when` to a numeric expression. Values <=0 hide both the
drawing and interaction; an omitted condition keeps the control visible. Conditions use the
same bounded validation as drawing expressions. Control width and height are at least 8 pixels.
Text supports `align: left|center` (default left) and `format: number|frequency|choice|tempo` (default number).
Frequency formatting uses two decimal kHz at >=1000 Hz and the declared numeric precision below.
The Delay example uses these fields for mutually exclusive tempo-step and millisecond controls.

`exp` and `log` expressions each take `value`; `log` is the natural logarithm. Domain errors
and non-finite results evaluate to zero, like other drawing expressions. These support a continuous
log-frequency band response in the Delay panel without changing DSP processing. Its plotted
high-pass and low-pass losses use a two-pole magnitude model at every sampled frequency.
Parameter text is fitted to its control's clip area, down to 8 logical pixels, and percent
readouts omit the space before the unit. Delay offsets have explicit text labels. Left-aligned Number widgets inset their text by four
logical pixels. Menu and ValueMenu widgets have a frame and inset their arrow four pixels
from the right edge; menu labels and arrows are vertically centered.
Keyboard and mouse focus use short white corner brackets around the complete custom control;
rotary controls include their caption inside that area. Pressing a control immediately requests keyboard focus and shows the corner brackets, including
before a drag crosses its movement threshold. Delay numeric fields use framed Number widgets.

## Choice menus

The `menu` gesture binds only X to a Choice parameter. A click, Enter or Space opens a popup
of its declared `ui.options`, with the current item selected. Choosing an item dispatches one
`device.setParam` command with its zero-based option index and ends the gesture. Opening,
cancelling with Escape, or clicking outside changes no parameter. Popups use the host overlay
layer and remain visible above or below the authored canvas. `text.format: choice` displays
the bound parameter's current option label instead of a number.

`ValueMenu` uses the same menu gesture with explicit `choices: [[value, label], ...]`.
Values are unique integers inside the bound parameter range; 1..128 entries with
nonempty labels of at most 128 bytes are accepted. It can bind a numeric parameter
without Choice options. Selecting an entry writes its value, not its array index.
Menus scroll when needed, using a fixed-width solid scrollbar that does not expand
on hover. Delay uses this node to expose actual note divisions,
triplet/dotted suffixes and sixteenth-note step counts while retaining saved values.

## Delay example layout

Delay uses independent SDK UI nodes throughout its 944 by 168 logical-pixel panel.
Including the host header and padding, it fits a 200-pixel device slot.
Time and Feedback occupy the left; Tone and Motion follow, with Mix at the far right. The only graph is Tone's filter response. All sections stay visible.

Left and Right timing columns flank the link icon in Time; the link button shares the
vertical center of the timing menus. Beats/ms buttons switch between
explicit note-length menus and millisecond fields. Link mirrors timing edits; each column has a synchronized offset field. Freeze sits
below Feedback and Ping Pong below Mix. Repitch/Fade/Jump remain in Time. Tone exposes filter enable,
centre frequency and octave width; Motion exposes rate mode/value, waveform,
delay/filter depth and morph. Speed appears once: a number in Hz/Time modes and
a ValueMenu in synchronized modes. The obsolete `lfo_expanded` parameter remains hidden for
saved-parameter compatibility and no longer affects presentation.

The host resolves widget text, surfaces, borders and accents from the active Suhina
theme at render time. No authored purple accent or device header is required.

## Reusable functional widgets

Dial also accepts optional `decimals` (default 0) and `format` (default `number`).
Format `tempo` displays BPM, or Auto for zero (a device may use that value to request tempo detection).
Formats `seconds`, `percent`, `decibels` and `balance` display milliseconds/seconds,
fractional percentages, dB and centre/L/R respectively. Existing Dial definitions
keep their previous readouts. The sampler uses these formats without changing
the saved parameter units.

`SampleWaveform` is a host-backed source view for the native sampler. It has no
parameter axis bindings. The host supplies decoded audio; start/end and loop
handles dispatch `device.setParam`. `SampleRecording` independently places the
input selector and Record/Stop actions (`device.recordSample`) beside other controls.
It registers a sample drop target for both browser and system file drops; the drop
runs `device.loadSample`. It renders an empty-source prompt when no audio is loaded.
This does not add a sample DSP node to serialized `.sd` graphs.

A control may specify a typed `widget` instead of a custom `drawing`. Widgets are
independent UI nodes; interactive nodes bind to a declared parameter (or two parameters
for the filter band display). They are not DSP nodes. The SDK supplies buttons,
number fields, knobs, ringed dials, steppers, disclosure toggles, choice menus,
waveform menus and filter band displays. Non-interactive Label, Panel and Separator widgets
use no parameter bindings and cannot receive focus. Dial follows the available
control size; its caption reserves two lines so knob centres stay aligned across
single-line and two-line labels; Stepper arrows adjust by one integer or the declared decimal precision,
while vertical dragging, keyboard edits and double-click reset remain available. Styling and bindings belong to each instance; no LFO section widget
exists. The host uses the same gestures, commands, keyboard handling, undo and
persistence as custom controls. Knobs use Suhina's small circular face, pointer and value;
the widget palette follows the active theme. Legacy drawing colours remain literal and legacy
drawing controls remain readable. A widget and a nonempty custom drawing are mutually exclusive.

`Panel { label }` provides a passive themed section surface, border and optional
small heading. An empty label gives a backdrop without a heading. Panel controls
have no parameter bindings and the host skips input interaction for them, so
numeric fields, menus and graph handles can be placed above a section surface.

Vertical/horizontal `drag` controls traverse the normalized parameter range over
400 logical pixels, independently of their rectangle's height/width. Shift keeps
the precision multiplier. Stock SDK numeric fields and knobs use the same device
parameter drag calibration. Position controls (envelope handles and XY pads) stay
mapped directly to their canvas positions.

`UiBinding.drag_scale` defaults to 1 and multiplies drag sensitivity only; it must
be finite within 0.01..100. Position mapping and keyboard steps are unaffected.
Double-click resets every parameter input, including menus and segmented buttons,
to its definition default. Passive widgets and host recording actions are excluded.
`WaveformMenu` accepts six standard shapes, optionally a seventh Wander shape;
its popup shows labelled icons. `wave { shape, cycles }` evaluates the shared SDK
waveform for previews (0 sine, 1 triangle, 2 saw up, 3 saw down, 4 square,
5 held random, 6 wander), with the normal expression validation limits.

Historical canvases without `SampleRecording` retain the inline recording strip in
`SampleWaveform`, so embedded definitions keep their input and recording actions.

`Envelope { attack, decay, level, release }` is a host-rendered interactive graph
with named parameter references and no X/Y bindings. Attack, Decay and optional
Release reference distinct time parameters starting at zero, with maxima at most
60 seconds. With Release, Level is 0..1 sustain; without Release, Level straddles
zero and controls a bipolar Attack/Decay peak. Missing/duplicate references and
incompatible ranges are rejected. Bounds must be at least 40 by 32 pixels.
The host draws the graph on the panel background with a thin theme-border zero-level
guide and no separate fill, with 8-pixel padding,
9-pixel editable points and 22-pixel hit areas. A cumulative logarithmic time axis
uses a 10 ms base and the sum of stage maxima; the ADSR note-hold segment occupies
one quarter of the plot. Zero-length stages meet at the same x position.
Point drags use displacement from the original press, not absolute hit-rectangle
coordinates, and emit shared parameter commands grouped for undo. Points support
Shift precision, keyboard arrows and double-click default reset. Readouts remain
separate widgets, so the graph's surface does not darken surrounding fields.

`SampleWaveform` supports anchored vertical zoom and horizontal pan on empty space.
Source/loop handles, slice lines and voice heads share its view transform. Peaks
are overlaid with solid two-pixel orange marker lines, without outlines;
source/loop handles use the same colour. The Slice by selector reserves room
for its full caption and arrow alongside the region and fade fields.
Peaks are rebuilt for the visible range, and areas outside S/E are gray. Manual lines
have horizontal resize cursors; double-click adds/removes a line. Line drags
preview locally without preparing an audio graph per frame; releasing commits
`device.setSampleSlices` once, as one undo step. Playback uses the committed map
until release, then new notes use the edited map. View navigation
never changes project data. The host resets the view when source audio changes.

`WaveformPreview { shape }` draws a theme-aware oscillator preview. Its X binding controls rate (linear or logarithmic), and `shape` references a zero-based waveform parameter (0..5 or 0..6). It uses a Position gesture with no Y binding. The SDK owns the drawing; device authors only supply the widget and bindings.
