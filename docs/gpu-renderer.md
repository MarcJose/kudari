# Drawing terminals on the graphics card

Kudari paints a terminal with `QPainter` into an image of its own and hands that image to
Qt Quick as a texture. This document plans a second way, for where Qt Quick draws through
its RHI (OpenGL, Vulkan, Direct3D, Metal): the graphics card draws the cells from a texture
that holds each glyph once. The painted image stays for the software renderer, for tests and
as a fallback. The steps below each leave a working, fully tested Kudari behind.

## Why

A comparison with seven other terminals in `tools/bench.sh` shows Kudari level with the
fastest on plain text, colors and `seq`, and behind on everything that changes much of the
screen in each frame. With the load `frames`, 300 full screens with colors in every cell,
Kudari takes 6.0 ms of CPU for each megabyte against 3.9 for alacritty and 4.8 for kitty.
The thread of the PTY costs the same for each megabyte as in alacritty; the difference is
drawing. A full frame of 315 × 93 cells costs 7–8 ms on the render thread, and Qt Quick
waits for it in the sync phase.

A profile of the render thread under that load:

| what | share |
|---|---|
| blending the masks of glyphs into the image (`QRasterPaintEngine::alphaPenBlt`) | ~16 % |
| filling the backgrounds of cells (`fillRect`) | ~7 % |
| copying the bands that changed to the graphics card | ~3 % |
| making the paints of the frame (`render::lines_into`) | ~2 % |
| colors (`QColor::fromRgb`), groups of glyphs | ~2.5 % |

Most of the time goes to the raster engine of Qt touching every pixel of every glyph on the
CPU. Tuning the code around it would gain a few percent. The terminals ahead of Kudari draw
on the graphics card, where blending a glyph costs next to nothing; at a scale of 2, with
four times the pixels, the gap grows further.

## Goals

- A full frame costs the render thread little more than making its paints: the target is
  below 2 ms for 315 × 93 cells, against 7–8 ms now, and the same at a scale of 2.
- The text looks the same: the glyphs come from the same fonts, laid out and rasterized by
  the same code of Qt as now, at the same places.
- Everything a frame shows works: colors, bold, italic, underlines of all kinds, strike
  through, box drawing and braille, wide characters, emoji, letters with marks, fallback
  fonts, selection, search matches, hints, the badge, a translucent background, fractional
  scales.
- The software renderer and the GUI test, which runs on it, keep the painted image.
- A setting turns the graphics card off, for a driver that draws wrongly.

## Not goals

- A text engine of its own: shaping, fallback fonts and rasterizing stay with Qt.
- Subpixel (LCD) antialiasing: Kudari draws grayscale text today and keeps it.
- Changes to the core: the paints of a frame stay as they are.
- Scrolling by moving the texture or other uses of the old frame: a frame on the graphics
  card costs so little that each is made whole.

## How it fits in

```
render::frame_into ──► Vec<Paint>  (Fill, Shade, Text; unchanged)
                          │
          RHI ◄───────────┴───────────► software renderer
          │                                   │
  quads for fills and shades          QPainter into the image of the
  quads for glyphs from the atlas     item, as now (bands, cursor image)
          │
  QSGGeometryNode × 2 under the item
```

`RkImageItem::updatePaintNode` decides by `window()->rhi()`, as it does for the texture of
the frame since that took only the bands that changed. With the RHI it builds two geometry
nodes instead of the image node: one for the backgrounds, lines and shades, one for the
glyphs. The cursor keeps its image and node of its own, over both; it is small and changes
alone.

### Fills and shades

`Paint::Fill` and `Paint::Shade` become two triangles each, with the color, and for shades
the alpha, in every vertex. Qt brings the material for that: `QSGVertexColorMaterial`, with
its shaders for every backend. Neighbouring cells of one color are one fill already, so a
frame has few of them.

### Glyphs

Each glyph is a rectangle that takes its pixels from an atlas texture. The atlas holds a
tile for each pair of font style and text that a frame showed: for nearly every cell one
character, else the text of a cell with its marks. A tile is made the first time it is
needed:

1. The glyph runs of the text come from the cache of glyphs that the painted path uses now,
   with the fallback fonts and the laid out texts it keeps.
2. `QPainter::drawGlyphRun` draws them in white onto a small transparent image, at the same
   place within a cell as on the screen, with room around for what reaches out of the cell,
   as italic letters and emoji do.
3. The tile is cut to the pixels the glyphs drew, and keeps where they lie from the point
   of the cell, so that space around a glyph takes no room in the atlas.
4. The tile goes into the atlas, which takes only the new tiles up to the graphics card, as
   the texture of the frame does now.

A glyph in a color then takes its shape from the alpha of the tile and its color from the
vertex. A glyph of a color font, an emoji, takes its colors from the tile. One small
shader does both: a flag in the vertex tells which. That shader is the only one Kudari
brings itself.

Drawing the tile with the same code that paints the image now keeps the shapes of the
glyphs exactly; what can differ is the blending, by a step of a color value at the edges.
The cells lie on whole pixels of the device, also at 125 %, so a tile fits every place its
glyph is shown at, and no glyph needs tiles for parts of a pixel. A text that a cell holds
with marks is one tile as a whole, so its marks keep their places.

### The atlas

- A texture of its own type, as the texture of the frame is: it keeps its storage on the
  graphics card and takes only the tiles that are new. Tiles go in rows, by height.
- One atlas for each thread that renders and each font key, as the cache of glyphs is
  kept now: font, antialiasing, hinting, emoji font and the density of pixels.
- 1024 × 1024 pixels to start, then twice as large up to the largest texture of the device.
  When it is full, it starts empty with the next frame and fills again from that frame.
- RGBA, so that color glyphs fit: 4 MB at the start. Tiles cut to their glyphs for every
  printable ASCII character in four styles take about 0.3 MB at a scale of 1, four times as
  much at a scale of 2.

### The badge

The badge stays a picture that `QPainter` draws, now into a texture of its own that changes
only with its text, in a node between the backgrounds and the glyphs, as it lies under the
text now.

### Translucent terminals

With an opacity below 100 % the background is not painted; what is under the window shows
through, as now. Fills of other colors and the glyphs blend as they do in the image.

### Updates

A change of the terminal builds both geometries anew from the paints of the whole screen.
For 315 × 93 cells that is about 30,000 glyph rectangles, 120,000 vertices of 24 bytes,
about 3 MB for the graphics card each frame under endless output; the vertex data of the
frame before is kept and overwritten. The painted path keeps its bands, its image and its
upload of the bands; the GPU path does not need them.

## Shaders

The shader of the glyphs is written in Vulkan GLSL, as Qt wants it, and compiled with `qsb`
of Qt Shader Tools into one `.qsb` file with the variants for SPIR-V, GLSL, HLSL and MSL.
The `.qsb` file is in the repository next to its source, with a script in `tools/` that makes
it again and a check in `tests/check.sh` that it matches its source, as for the other files
that tools make. Builds then need no Qt Shader Tools, also not on Windows.

## Settings

`Appearance/Renderer` with the values `auto` (the graphics card where Qt Quick draws through
the RHI, the default once it is done), `gpu` and `cpu`. It takes effect for the next frame.
Until the last step, the default stays `cpu`.

## Tests

- The pixel checks of the GUI test run on the window that draws through the RHI (Mesa in
  software, as in the CI) with the GPU path: what the window shows against the same frame
  painted with `QPainter`, with a tolerance of one step in each color value for the
  blending, and none for where glyphs are.
- The catalogue of those checks covers what the goals list: styles, underlines, box drawing,
  wide characters, emoji, marks, fallback fonts, a selection, matches, hints, the badge, a
  translucent background, at 100 %, 125 % and 200 %.
- A test of the atlas: tiles that do not overlap, growth, starting empty when full.
- The bench, A/B against main: `frames`, `unicode`, `boxes`, `plain`, `typing` and `scroll`,
  at a scale of 1 and 2; and the scenarios for memory on the graphics card.
- On Windows, in the VM through Direct3D: a frame with the catalogue against the painted one.

## Steps

Each is one pull request; the setting keeps the painted path as the default until step 5.

1. **Fills.** The setting, the switch in `updatePaintNode`, the geometry of fills and shades
   with `QSGVertexColorMaterial`; glyphs still through a painted image on top. A pixel check
   for the fills through the RHI.
2. **Glyphs.** The atlas, the shader and its `.qsb`, the glyph geometry; the painted image
   goes away on this path. Pixel checks of the catalogue against the painted frame.
3. **The rest.** The badge, translucent terminals, color glyphs, fractional scales, the
   atlas when full; Windows in the VM.
4. **Measuring.** The bench and the scenarios A/B at scales 1 and 2, and what they show to
   improve.
5. **Default.** `auto` as the default, the documentation, and the measurements in the
   description of the pull request.

## Risks

- **Drivers.** A driver can draw wrongly or slowly; the setting `cpu` is the way out, and
  the software renderer stays as it is.
- **The RHI.** Kudari already uses its semi-public API for the texture of the frame; the
  geometry nodes and materials of Qt Quick are public.
- **Text.** Blending on the graphics card can differ from the raster engine of Qt by a step
  of a color value. A difference larger than that is a fault of this path, and the pixel
  checks catch it.
- **Memory.** The atlas adds some MB on the graphics card; the image of each terminal goes
  away on this path, which gives back more at a scale of 2.
