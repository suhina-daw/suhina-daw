---
description: Experimental live modal backdrop and optical coatings for track and clip surfaces
read_when: glass effect, backdrop blur, rendering materials, Aero
type: plan
status: draft
last_verified: 2026-10-22
covers:
  - crates/ui/src/kit/**
  - apps/suhina/src/main.rs
  - apps/suhina/**
---

# Glass rendering experiment

## Optical direction

Public references: [Materials](https://developer.apple.com/design/human-interface-guidelines/materials)
and [Meet Liquid Glass](https://developer.apple.com/videos/play/wwdc2025/219/) describe
transmission, edge lensing and directional highlights rather than a full-body bevel.
The implementation is independently written; no reference shader or asset is imported.
For dense musical content, use a quiet centre and a narrow curved rim: a directional
outer highlight, an inner dark band and a weaker opposing light return. Scale these
down with row height and keep them inside the existing editing rectangle. Track and clip lenses refract a shared current-frame substrate of surface bodies and
gradients. Floating panels refract the current UI background through the same bounded
edge lens, independently implemented in the shared kit.

[Microsoft Acrylic](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic)
also builds depth from layered background, blur and tint. In the dense content layer,
combine a source-coloured body gradient with the rim rather than replacing the body
with edge lighting alone. The body has a broad upper reflection, a shaded lower body
and a weaker light return at the bottom. The reflection is slightly directional,
without a hard glossy stripe across notes or waveforms.

## Current state

The original static experiment below remains available. There is also an experimental
native integration, disabled by default, with per-surface global strengths under
Appearance › Liquid (experimental), and optional overrides saved with each theme in
Edit current theme. Use global liquid settings removes the theme override.

Run with `--renderer wgpu` and raise a strength above 0 to opt in. The old
`SUHINA_LIQUID_PREVIEW` environment switch is no longer needed.
`--new-instance --demo` opens a separate review window without replacing the running project.
Do not force-close the user's existing project to try it.

`apps/suhina/src/liquid.rs` replays this frame's already-painted egui shapes into its own
offscreen renderer **before** the Settings modal and dimmer are painted. The source and
horizontal intermediate are sRGB attachments, sampled in linear light; the final vertical
pass explicitly encodes the result for egui's gamma-encoded native-texture contract.
The blur is separable, 19 taps per direction, sigma 3 downsampled pixels. The longest target
side is bounded at 1024 physical pixels. Three targets, sampler and bind groups are retained
until their dimensions change; the main renderer's blurred and sharp texture IDs are reused
when resized. The sharp view reinterprets the sRGB source as unorm so egui samples the
encoded colour values without a second decode; no additional capture or render target is needed.
No swapchain readback, previous frame or recursive rendering is used.

The UI-facing provider and independent Panel/Track/Clip options live in
`crates/ui/src/kit/liquid.rs`. Settings uses the live blurred backdrop under a theme tint. A 121-vertex, 200-triangle
edge mesh samples the same frame's sharp source with smoothly displaced UVs. The lens
band is at most 32 logical pixels and no more than a quarter of the shorter dimension;
maximum displacement is 18% of that band, scaled by Panel strength and small-surface
fading. Coordinates remain monotonic (no folded image), clamped to the captured screen.
The centre has zero displacement and zero sharp-overlay opacity. All labels and controls
are painted afterward. Missing sharp sources fall back to the blurred current backdrop.
Arrangement and Launcher track headers and clip backgrounds get a lightweight optical
coating (a narrow rounded rim with directional highlights, an inner shade and a
weaker opposing light return, over a source-coloured directional body gradient),
painted before labels, controls,
notes and waveforms. A second, lazy GPU capture renders only visible surface bodies and their theme gradients,
excluding labels, notes, waveforms and controls. Deferred edge meshes are inserted before
foreground at the original paint positions. One substrate serves all visible track and
clip lenses. It has independent targets/texture IDs from the modal capture and skips the
Gaussian passes: content lenses use the sharp encoded source. There are no per-clip
captures. Temporarily hiding content lenses during modal replay keeps the dialog's backdrop free of
glass drawn from another capture; the lenses are restored afterward. Each capture renderer
binds the textures it meets from where they already are, without copying them: egui's
textures (images, icons) from eframe's main renderer, and another capture's image from that
capture's targets. A capture never samples its own target, and what it cannot bind (a native
callback, a texture only the main renderer can draw) is left out while the rest still draws.
Menus (the menu bar's, right-click menus and dropdowns) get a third capture: once the frame is
painted, the window's own layers are drawn and blurred into targets of the menus' own and
placed under each popup's frame, fading in with it; in a see-through window the pixels under
the menu are cleared first, so the image keeps the window's alpha.
Surfaces smaller than 12 logical pixels stay solid. The three strengths are independently
saved percentages: 0 is off, 100 is the strongest supported material. Panels interpolate
from opaque to a 55% theme tint at full strength; track and clip rims keep the source
hue and draw all text/notes/waveforms sharply on top. Rim intensity fades with the
shorter surface dimension between 12 and 40 logical pixels. Each rim is bounded to
216 vertices plus 10 body-gradient vertices, regardless of clip length; the rim's
transparent centre reveals the body gradient. All geometry stays inside the editing
rectangle. Missing settings default to 0.

The wgpu host installs its provider even while all strengths are 0; no backdrop capture
is requested until a liquid strength is raised; Track/Clip strengths request only the
shared content substrate, Panel strength requests the modal capture. OpenGL has no provider and disables the controls.

OpenGL, disabled animations and the built-in high-contrast theme keep solid surfaces.
The accessibility gate is checked after running settings/theme changes too. If replay
would require an unsupported image texture or a foreign paint callback, the panel falls
back to solid rather than displaying an incomplete or stale backdrop.

Remaining costs/limits: capture currently clones/tessellates UI shapes and copies/compares
the current CPU font atlas, uploading the atlas only when it changes. This is UI-thread
work, never audio callback work, but needs profiling and a more efficient atlas update
path before the material can leave its experimental status. Foreign image support, playback/scroll/zoom
measurements, interactive visual QA on more hardware are still outstanding.
A GPU-only blur does not imply that the whole capture is free.

The native hardware test was run explicitly on Windows: constant RGB preservation,
linear-light checkerboard mean, impulse symmetry and a changed frame at a new size passed.
GPU readback exists only in that test, not in the application path. Kit tests cover
default/tiny/reduced-effects solid rendering, removal of an old backdrop on failure,
bounded premultiplied rim geometry and intensity scaling at small sizes.
The body-gradient regression also checks shading on light and dark source colours,
coverage between the rims and a solid fallback for invalid/disabled inputs.
Settings-window (7, including live independent strength controls and persistence) and constrained-pane (16) regression tests also passed, as did Clippy
for both the application and UI with all targets. These checks do not replace visual QA
or playback performance measurements.

```sh
cargo test -p suhina --bin suhina liquid::tests -- --include-ignored
cargo test -p suhina-ui --lib kit::liquid::tests
```

The two-pass shader in `crates/ui/src/kit/glass_blur.wgsl` now runs in the standalone
`glass_preview` native example. It uploads a fixed synthetic backdrop once, keeps three
768 by 432 RGBA textures and re-runs the horizontal/vertical GPU blur only when Blur changes.
Controls are drawn sharply over the material; Tint and a solid fallback can be compared.
The example built and its native window started on Windows with no GPU validation error
reported in stderr. This older example has not been re-profiled or connected to the DAW;
the native integration above uses its separate `apps/suhina/src/liquid.wgsl` instead.

Run `cargo run -p suhina --example glass_preview` from the worktree. No new dependencies were added.
The preview uses fixed-size gamma-space RGBA textures to match egui native texture support;
it does not yet establish physically correct linear-space or translucent-source handling.

## Integration requirements

1. Render the content behind the glass into a separate texture before compositing chrome.
   Decide which regions are behind each panel; simply blurring the previous screen frame
   causes stale content and feedback.
2. Downsample that backdrop and blur into two reusable textures, never reading and writing
   the same texture in one pass. Use a linear clamp sampler. Expand each captured region
   by the kernel footprint so edges do not develop seams.
3. Composite the blurred backdrop through the panel mask, then apply theme tint, opacity
   and a restrained highlight. Draw text and controls afterward, sharply.
4. Reuse allocations while size and renderer stay the same. Resize/recreate only off the
   audio thread. Do not use a per-frame CPU screenshot/readback path.
5. Start opt-in on stationary chrome with bounded affected area. OpenGL initially uses a
   solid fallback; implement its blur only after the wgpu experiment is measured.
6. Keep material settings independent from color themes and layout. Missing settings mean
   solid materials. Reduced effects/high contrast must force a readable solid fallback.

## Verification gate

Validate WGSL and test constant colors, an impulse and a checkerboard on the GPU. Check
linear color space and premultiplied alpha, resizing, high DPI and stale-frame artifacts.
Measure frame time during playback, scrolling and zooming against the solid baseline.
Only then connect the effect to a user-facing Appearance setting and build a preview.

## Smooth versus stepped experiment

The smooth mode uses a bounded 49-sample Gaussian in each direction (sigma at most 8).
This avoids the sparse-sample ghost bands of the earlier large-radius experiment. A separate
Stepped texture toggle retains the five-sample sparse kernel as an explicit artistic option.
Both modes run only on a blur/mode change in the static preview. The denser kernel needs GPU
measurement before use on a live UI; no frame-rate claim has been established.

## Editable theme gradients

Edit current theme provides independent panel, track and clip overlays. Each has a
toggle, clockwise direction, live preview and 2–8 RGBA stops with positions, opacity,
add and remove controls. The optional theme `gradients` object follows saves, copies,
exports and Reset changes. A gradient is independent of the liquid strength and GPU
availability. Reduced effects and high contrast still suppress surface materials.

The kit clips the rectangle against each pair of stop planes and triangulates those
polygons. Intermediate stops remain exact at diagonal angles without a fixed-resolution
grid. Colours interpolate premultiplied RGBA. Geometry is bounded by eight stops, and
all polygons stay within the surface. This changes appearance only, never project RGB
colours or creation defaults.

Private background image decoding, filtering and placement live in separate modules under
`crates/ui/src/app/background/`. The decoded source is retained while blur or window size
changes. One bounded worker owns filtering; only the current request may publish a texture.
Blur uses premultiplied alpha and is converted from logical display pixels to texture pixels.
Settings uses its original theme base, not the main-window opacity-scaled colour.
While a private image is displayed, surfaces use ordinary blending over the image rather than
replacing its pixels with desktop transparency. Removing the image restores replacement fills
on an alpha-capable host. Each liquid capture keeps only the shared textures used by its current
render; superseded sources and their renderer registrations are released together.
The removed experimental Mica key is accepted and ignored when reading old preferences.

## Desktop-aligned wallpaper: platform investigation (not implemented)

A wallpaper-only background needs two independent inputs: the image and its placement on
an output, and the client area's location in that same coordinate system. A compositor
blur-behind effect is not a substitute: it can include other windows or desktop icons.
The current implementation still renders an explicitly chosen image fitted to the window.
No automatic wallpaper discovery or desktop alignment is shipped by this change.

Public API findings checked on 2026-10-22:

| Platform | Image and layout | Window position and limitation |
| --- | --- | --- |
| Windows | `IDesktopWallpaper` exposes per-monitor image paths, monitor rectangles and placement mode. | Native window position is available. Mixed DPI, spanning and disconnected displays need explicit handling. |
| macOS | `NSWorkspace.desktopImageURL(for:)` and desktop image options describe each screen's image and layout. | Native window/screen coordinates are available; convert the coordinate origin and backing scale. Query AppKit on the main thread. A dynamic wallpaper is not necessarily a single current image file. |
| X11 | Read the desktop environment's wallpaper configuration; X11 itself does not standardise a wallpaper file path. | Translate the client origin to root coordinates. Do not capture the root window, which can include unrelated content. |
| Sway/Wayland | Wallpaper source depends on the wallpaper program and configuration. | Documented IPC `GET_TREE` and `GET_OUTPUTS` provide geometry; combine container and client offsets and output scale. |
| Plasma/Wayland | Plasma wallpaper configuration is separate from window geometry. | A KWin integration can expose client geometry and change notifications. The private plasma window-management protocol explicitly excludes ordinary applications and must not become a direct dependency of Suhina. |
| GNOME/Wayland | GNOME background settings identify static wallpaper sources. | Mutter exposes geometry inside the shell; a Shell integration is a candidate bridge, not an API an ordinary application can simply call. |

The XDG Wallpaper portal sets a wallpaper; it does not provide a getter for the current
wallpaper or the window's global coordinates. Winit reports global position as unsupported
on Wayland. Neither fact proves that desktop-specific integrations are impossible.

A future platform adapter should supply image identity, placement mode, output bounds and
scale, and the client rectangle, with explicit availability. Keep crop calculation and
filtering platform-independent. Moving a window should update crop coordinates without
redecoding or reblurring the image. Multi-monitor windows may need multiple image regions.
Do not silently describe a window-fitted image as desktop-aligned. Desktop helpers and their
installation, permissions and Rust-only compatibility still need a design decision; they
have not been implemented or tested here. Animated backgrounds remain separate extension
work under ADR 0015.

Sources:

- [Windows wallpaper API](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-idesktopwallpaper)
- [AppKit desktop image](https://developer.apple.com/documentation/appkit/nsworkspace/desktopimageurl(for:))
- [Xlib coordinate translation](https://xorg.freedesktop.org/X11R7.0/doc/PDF/xlib.pdf)
- [Sway IPC specification](https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd)
- [KWin scripting API](https://develop.kde.org/docs/plasma/kwin/api/)
- [Plasma private protocol warning](https://github.com/KDE/plasma-wayland-protocols/blob/master/src/protocols/plasma-window-management.xml)
- [Mutter window geometry](https://gnome.pages.gitlab.gnome.org/mutter/meta/method.Window.get_frame_rect.html)
- [XDG Wallpaper portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Wallpaper.html)
- [Winit window positioning](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html)

Clip optical strength is limited to 35% of the full panel/track range, across body reflection, rim and refraction together. The 0–100% clip slider still spans its entire range; even at its maximum, repeated clip boundaries should remain subdued rather than resembling raised buttons. Explicit theme gradients remain independent.
