# PatternForge — user guide

*Version française : [GUIDE_UTILISATEUR.md](GUIDE_UTILISATEUR.md).*

A 2D CAD workshop for drafting precise sewing and leather patterns, in millimetres, offline. This
guide describes what the current version does and what it does not do. The interface starts in French;
switch with **View › Language**, or start with `--lang en`.

> **Before printing for real**: PDF export remains *experimental* until the 100 mm square has been
> measured on paper (see “Check your print”).

**Not covered by this guide yet** (the features exist in the application): seam allowances (piece card,
“Seam allowance”), darts, pleats and flares, variables and formulas, sizes and grading, measurement
tables and client records, variants, bill of materials, cutting plans and automatic placement, stitching
lines and leather zones, the piece library, named versions, direct printing and projection. The command
line is described in [CLI.md](CLI.md) (in French).

## 1. Getting started

```bash
cargo run --release -p pf-desktop
```

Options: the path of a project to open; `--demo` (reference exercise); `--lang fr|en`; `--theme dark`.
Project files are `.pforge.json` (readable, versioned text).

## 2. The screen

| Area | Role |
|---|---|
| Menu bar | File, Edit, View (snapping, theme, language), Help |
| Toolbar | Select, then one menu per family (Draw, Construct, Modify, Piece, Measure) listing the tools with their letter; Undo, Redo and Fit stay on the right; a single row |
| Left panel | Project, sheets, pattern piece |
| Canvas | Drawing to scale; wheel = zoom around the cursor, middle button or Space + drag = pan, `Home` = fit everything |
| Inspector (right) | Properties of the selection, measurements, closed outlines |
| Status bar | Tool, mouse position in mm, zoom, contextual help |

## 3. Sheets

A project holds several independent **sheets** (“+ New sheet”, `F2` or double-click to rename, arrows to
reorder). A sheet is not an A4 page. It can carry one or more pattern pieces (name, quantity, material):
the “This sheet holds a piece” box creates a piece card without an outline, and the “Extract a piece”
tool creates a piece with its outline (see below). Pieces are saved in the project; older file versions
open and are migrated.

**Duplicate a sheet**: the panel's “Duplicate” button (or `File › Duplicate sheet`). The copy comes right
after the original and becomes the active sheet; it takes points, paths, notches, dimensions, groups,
layers (locks included) and the linked piece, all independent of the original. Undone in one step with
`Ctrl+Z`.

**Import a sheet from another project**: `File › Import a sheet from a project…`, choose the
`.pforge.json` file (older files are migrated), then the sheet to copy. It is added last in the current
project (the name is made unique: “X (2)” if it already exists); the source project is not modified.
Undone in one step.

### Layers

The “Layers” panel (left column, under the piece) sorts the points and paths of a sheet. “+ Layer”
creates one and makes it **active**: everything you draw afterwards (including snapped or constructed
points) goes into it; “Default layer” is the starting layer. Click a name to make it active; the “Name”
field renames the active layer; ⏶ ⏷ change its rank; “Delete” refuses a layer that still holds objects
(move them first). “Move selection here” puts the selection into the active layer.

- **Show**: a hidden layer is not drawn, snapped to, selectable or exported (SVG/PDF).
- **Lock**: a locked layer stays visible and snappable, but its objects can no longer be modified, moved
  or deleted (an error message, nothing changes). A point belongs to the layer where it was created; a
  locked path also protects its two ends.

Everything can be undone. Notches and dimensions follow the layer of their path or points. Not yet:
layer colour, drawing order by layer.

### Names and groups

- **Name of a path**: select a path, “Name” field of the inspector (applied on Enter or when leaving the
  field). Optional; a name can only be used once per sheet; clearing the field removes the name.
- **Group**: select at least two paths, `Edit › Group` or `Ctrl+G`. Clicking a member selects the whole
  group (same with the selection rectangle); `Alt+click` selects only the member. Moving, rotating,
  mirroring and duplicating (`Edit › Transform selection`) apply to the whole selection as a single
  undoable step.
- **Ungroup**: `Edit › Ungroup` or `Ctrl+Shift+G`. A path belongs to one group at most; deleting a member
  removes it from the group, an empty group disappears. A path on a locked layer cannot be grouped.
- A copy (duplication) is not grouped and does not take the original's name.

### Find an object

`Edit › Find…` or `Ctrl+F` opens a window: type part of a name (case and accents do not matter), then
refine by type (path, segment, curve, notch, dimension, point), by role (paths only) and by layer.
Clicking a result selects the object and frames the view on it. The search covers the active sheet;
objects on a hidden layer are not listed.

### Cut, copy, paste

`Ctrl+X`, `Ctrl+C`, `Ctrl+V` (or `Edit › Cut / Copy / Paste`) act on the selection, between sheets and
between projects (a fragment is also placed on the system clipboard, so you can paste into another
PatternForge window). Pasted objects get new identifiers, go to the **active layer** and are selected;
pasted at the same place on the same sheet, they are offset by 5 mm (as many times as needed). Kept:
constructed points whose parent path is also copied, notches, dimensions whose two points are copied,
fully selected groups. A point whose parent is not copied becomes **free** (a notice says so); a
perpendicular or parallel whose reference is not copied only loses the memory of its construction. Pieces
are not copied. Pasting into a locked layer is refused. Cut is refused if an object is on a locked layer.
Pasting is a single undoable step. Inside a text field these keys keep their usual role. The “Paste” menu
item only uses the internal clipboard; pasting from another window is done with the keyboard.

## 4. Tools

Pick a tool in the toolbar or with its letter; `Esc` cancels the current gesture and leaves nothing
behind. Type numbers with a comma or a point, with an optional unit (`120`, `12.5 cm`, `1.2 m`); `Tab`
moves to the next field, `Enter` confirms.

| Tool | Letter | Use |
|---|---|---|
| Select | `V` | Click, Shift+click (add/remove), selection window; drag a point to move it |
| Point | `O` | Places a free (or snapped) point |
| Segment | `S` | Start, then typed length and angle, or click the end |
| Rectangle | `R` | Typed width and height, or two corners |
| Perpendicular | `P` | Reference segment, start point, length; “Flip” changes side |
| Parallel | `L` | Reference segment, typed distance; the side follows the mouse |
| Point at distance | `D` | A point on a segment at a given distance from its start |
| Notch | `N` | Notch on a path at a distance from its start; length, side, double notch |
| Grainline | `G` | Like a segment; drawn with a double-headed arrow |
| Curve | `C` | Start, end, then drag two handles; the inspector gives the developed length |
| Smooth curve | `B` | Successive clicks on the through points (usual snapping); `Enter` finishes (2 points minimum), `Esc` cancels |
| Circle | `E` | Four modes in the toolbar: **center + radius**, **3 points**, **2 points + radius** (the side of the center follows the mouse) |
| Arc | `A` | **Center + angles**: center, then radius and start angle, then end angle; **counterclockwise** by default, “Flip” takes the other arc. Also: 3 points, and 2 points + radius |
| Fillet | `F` | Click a first path **on the side to keep**, then the second; type the **radius**, `Enter`. Paths are shortened to the tangent points (a circle is never cut). A radius that is too large, or paths that do not meet, is refused with the reason. Notches in the removed part are deleted with a notice |
| Chamfer | `H` | Like the fillet, between two segments; one distance (or two, `Tab`), `Enter` |
| Tangent | `T` | Click a point then a circle or arc: tangent line, the clicked side picks the solution. Click a curve: segment tangent at the aimed point, typed length (50 mm by default). A fixed construction: it does not follow later changes |
| Measure | `M` | Distance between two points (read only) |
| Dimension | `Q` | Click two existing points; the offset (10 mm by default) is typed or edited in the inspector. A **displayed measurement** that follows the points, not a constraint; also exported to SVG and PDF |

**Circles and arcs**: a circle's radius is a *creation parameter* (editable in the inspector, not
enforced); circumference, area, exact arc length and arc radius are *measurements*. An arc's center and
ends are points: dragging the center moves the arc, dragging an end changes its radius. A circle or
closed arcs form an outline (perimeter, area). Notches are placed at a distance along the arc. Mirroring
reverses the direction. With two possible intersection solutions, the one closest to the mouse is kept.
Arcs are exported as true arcs (SVG) or as Bézier curves with an error below 0.005 mm (PDF).

**Intersection of two paths**: with the Point tool (`O`) and snapping on, click a crossing between a curve
and a segment, another curve, an arc or a circle: a constructed point is created at the exact
intersection (1e-9 mm precision, tangency included). Paths are not extended. The point follows its paths:
if you move them so that they no longer cross, the move is **refused** with a message.

**Relative points**: the Point tool offers modes that create a **maintained** point (it follows its
parents): *On curve* (click the path, type the arc length; “Measure from the end” box), *dx / dy*,
*Midpoint*, *Projected*, *Bisector*, *Direction* and *Direction / path*, *Compass* (centers A then B,
typed radii, then a click near the wanted solution). Values can be edited later in the inspector. A
change that would make the construction impossible is **refused** and nothing moves. These points cannot
be dragged; deleting a parent point is refused while the relative point exists.

**Divisions**: *Division* mode of the Point tool. Click a path, then type either a number of parts (`5`)
or ratios (`1:2:1`, `1/4;1/2;1/4`, `0.5 1.5`) and press Enter: N − 1 points are created at once (one Undo
removes them). Parts are equal in **arc length**; on a circle they are equal in angle. These points are
**maintained**: if the path changes, they keep their fraction. N ranges from 2 to 1000.

**Guides** (*Guide* tool, `U`). A guide is an **infinite** line (horizontal, vertical or oblique) used as
a landmark: it is not a path, is not exported by default and belongs to no piece. Modes: *Horizontal* or
*Vertical* (one click, or type the position then Enter), *Oblique* (click a through point, then type the
angle, or click a second point). It is drawn as a thin dashed blue line and can be **snapped** to,
including guide/guide and guide/path intersections. Select it to inspect it (through point, angle,
*symmetry axis* box, layer); drag it to move it; Delete removes it. View menu: *Guides* and *Rulers*.

**Modify a path** (*Extend* `I`, *Split* `K`, *Merge* `J`; Edit menu; *Reverse path direction* on the
selection). Each operation is one Undo. *Extend*: click a segment or arc near the end to change, then
type the value (positive: extend; negative: shorten), or click the path to reach. *Split*: click the path
where wanted (a smooth curve splits at one of its through points, a circle does not split): two paths with
the same layer, role and group; notches stay exactly in place. *Merge*: click two paths that touch at a
shared end and are of the same type, layer and role. *Reverse*: start and end are swapped, shape and
notches do not move. Points constructed on a modified path become free (message); a lost notch is
reported. Impossible cases are refused with a message.

**Smooth curve**: the curve passes exactly through its through points. Each point is **smooth** or a
**corner** (“Corner” box; not at the ends). Tangents have a computed default; to change them, type a
**magnitude** (mm) and a **direction** in the inspector (“Recompute” restores the default). Tangents are
not dragged with the mouse. “Insert after” adds a point, “Remove” takes one away (two points minimum).
The total and per-section developed lengths are **measurements**; nothing is enforced. The guaranteed
continuity is tangent continuity (C1), not curvature.

**Snapping** (View): ends, intersections, midpoints, points of a path. The marker shows the kind of
target; snapping picks a target, it never distorts the computed value.

**Edit › Transform selection**: move, rotate, mirror, with or without a copy. Notches follow their path;
mirroring reverses their side.

**Edit › Align / Distribute / Make horizontal or vertical**: act on the selected points (and the ends of
selected paths). *Align* to a side, to the median line, or onto the selected guide or segment.
*Distribute*: equal spacing. *Make horizontal / vertical…*: the selected segments rotate together with
everything attached to them. A constructed point that would have to move or a locked layer makes the
operation impossible: a message says so and nothing moves. A single undoable step.

**Linked symmetry** (Edit › *Linked symmetry…*, then *Unfold symmetry*): draw one half, mark a guide as
*axis*, select the points and paths to mirror **and** the guide, then choose *Linked symmetry…*. The other
half is created: its points are **maintained** (they follow the original and the axis), with the same
roles, layers and mirrored notches. A point on the axis is shared by both halves. Curve handles and
notches on the mirrored side are **copied at creation** and do not follow later edits. *Unfold symmetry*
makes the two halves independent.

**Offset a contour** (*Offset* tool, `X`, Edit menu): creates a path **parallel** to a chain of
end-to-end paths (open chain or closed outline; a circle offsets alone). Click a path: the chain is found
through shared points. Type the distance or let it follow the mouse; **the side follows the mouse**,
*Flip* takes the other side. *Outer corners*: *Mitre* or *Round*. Inner corners are always trimmed and
loops removed. The role of the result is chosen in the palette (*Construction* by default). An offset that
would cancel the outline, a zero distance or a branched chain is refused. **The result is a new
independent path: it is not recomputed if the original changes.** The distance to the original is the
typed one (to about 0.00002 mm on curves), except at outer mitres and bevels, which are farther by
nature.

## 5. Three kinds of values

| Kind | Example | Behaviour |
|---|---|---|
| Observed measurement | length, angle, developed length | Read only, locks nothing |
| Creation parameter | “120 mm at 0°” of a segment | Kept for information, **not maintained** if a point moves |
| Construction parameter | distance of a point placed on a segment | Maintained: the point follows its segment |

There is **no** general constraint solver. Only explicit links are maintained: constructions you choose to
keep, formulas, and the imposed length of a curve. Nothing follows automatically otherwise. Deleting the
parent of a constructed point “freezes” it (a notice is shown).

## 6. Path roles

Construction, cutting outline, sewing line, grainline, fold, axis, then the special lines (topstitching,
center front, center back, waist, hip, bust, skiving and gluing lines): a role is told apart by colour
**and** by line pattern and width, on screen and on export. Choose the role in the “PATH ROLE” list of
the inspector (one undoable step; refused if the path's layer is locked). Simple closed outlines give
their perimeter and area in the inspector. See also “Special lines and linked topstitching” below.

## 7. Saving and undoing

`Ctrl+S` / `Ctrl+Shift+S` save (atomic write: a crash does not destroy the file), `Ctrl+O` opens,
`Ctrl+N` creates. `Ctrl+Z` / `Ctrl+Y` undo and redo; a complete gesture is a single step. A refused file
(unknown version, missing reference) opens with a clear error, with no silent loss.

**Autosave.** Every 60 seconds (if the project changed), a backup copy is written next to the file:
`myproject.pforge.json.autosave`. A project never saved is copied to `%APPDATA%\PatternCraft\recovery\`
(Windows). Your file is never overwritten by this copy; `Ctrl+S` deletes it. After a crash, a box offers
to **Recover** (the project comes back as modified: save to keep it) or **Ignore** (the copy is deleted).
An unreadable copy is renamed `.corrupt`. The interval is set with the `autosave_seconds` key of
`settings.json` (`0` = off).

## 8. Exporting

File menu: export to **SVG** (millimetres, exact dimensions) or **PDF** (1:1; pieces larger than the
printable area are split into pages with a 10 mm overlap and `L#C#` marks, row and column). By default the
PDF is **A4 portrait with 10 mm margins**. Page 1 is a calibration page: 100 mm square, ruler, legend.
PNG and JPEG export is available from the command line.

### Page size, orientation and margins (PDF)

File › **PDF page setup…** sets the paper; the setting is remembered **on this computer** (application
preferences, not in the project).

- **Format**: A4 (default), A3, A0, Letter, **Custom size** (190 to 5000 mm each way) or **Roll** (width
  190 to 5000 mm).
- **Orientation**: portrait or landscape. No effect on a roll.
- **Margins**: 0 to 100 mm each: the area your printer cannot print. The remaining printable area must be
  at least 160 mm wide and, except on a roll, 170 mm high. **Enter your printer's real margins**:
  PatternForge does not read them from the driver.
- **Preview**: page size, printable area, number of pages. An invalid entry is explained in red and
  “Apply page setup” stays disabled.
- **The scale is always 1:1**: a pattern larger than the printable area is **split into pages**, never
  shrunk. A **roll** receives each sheet as **one continuous page**; if the pattern is wider than the roll
  or longer than 20,000 mm, export is **refused**.

### Page assembly

In the same window, **Page assembly** controls how a large pattern is split. By default (10 mm overlap,
boxes unchecked) the output is unchanged.

- **Overlap (mm)**, 0 to 50: width of the strip two neighbouring pages share.
- **Registration marks**: four small **crosses** in each overlap strip; laying one page on its neighbour,
  they must coincide exactly. Each edge shows the neighbouring page, each page its own identifier.
- **Assembly plan**: an extra page after the calibration page, a **thumbnail of the page grid**; it is
  **not at 1:1** and says so.
- **Save paper**: pages **with no line at all** are skipped (the others keep their identifier), and the
  sheet may be **rotated by 90°, 180° or 270° in the PDF only** if that reduces the page count. Your
  pattern is never modified. Rotation is refused when a grainline would exceed its piece's rotation
  tolerance.
- The window gives the **number of pages before exporting**.

Not provided: A5, A2 and A1 formats; per-piece rotation within a sheet. **The scale actually obtained on
paper is not validated**, nor is the alignment of the marks: print the calibration square in your format
and measure it.

### Check your print

1. File › **Export 100 mm calibration square as PDF…** (or open `examples/etalon-100mm.pdf`) with the
   page format you intend to use.
2. Print at **100 %** (never “fit to page”).
3. Measure the square with a steel rule: 100 mm expected.

Until you have made this measurement on your printer, do not cut material on the strength of the export.

### Calibrate your printer (X / Y scale correction)

If your printer slightly shrinks or stretches the drawing (the 100 mm square measures 99.5 mm, say), you
can **correct** it: the PDF is then drawn at the inverse scale. The profile is **per printer** and
remembered on this computer.

1. Export and print the calibration page with **no active profile**, at 100 %.
2. Measure the square: **X** = horizontal side, **Y** = vertical side of the printed page.
3. File › **PDF page setup…** › **Calibrate printer…**: name the profile (40 characters at most, 16
   profiles at most) and enter both measurements in mm (to 0.01). Factors are shown **before saving**:
   factor = 100 / measurement (99.5 mm → 1.005025). A factor must stay between 0.9 and 1.1.
4. Save the profile, then activate it. Profiles can be edited, disabled and deleted.
5. With an active profile the whole pattern is scaled by the factors, the page count accounts for it, and
   each pattern page states the correction.
6. **Check on paper**: print the calibration page again and measure. The **corrected** square must measure
   100 mm.

**Reinforced calibration** (on by default when a profile is active) adds two graduated rulers on the
calibration page and two 100 mm rulers on each pattern page. These rulers are **always in real paper
millimetres, uncorrected**, so you can measure the printer even with a correction active.

**What the software does not know**: it never claims the printer is “calibrated”. The factor is a setting
computed from **your** measurement; verify it with a rule on the printed paper. The correction applies to
PDF only, not SVG.

### Output content (what is written to PDF and SVG)

File › **PDF page setup…** › **Output content**. Each box shows the number of elements on the current
sheet. Untick what you do not want, then **Apply page setup**: the choice applies to **all sheets**, to
PDF **and** SVG, and is remembered on this computer; the project file does not change.

- **Outlines**: cutting, sewing, grainlines. “Sewing only” and “Cutting only” are shortcuts. A *cutting
  outline* here is a path you drew with the cutting role; the cut outline of a piece with a seam allowance
  is computed from its card. If a sheet has no cutting path, “cutting only” does not invent one.
- **Lines**: construction, folds and special lines, axes and guides.
- **Marks and markings**: notches, drill holes / punch marks / crosses, buttons and buttonholes, points,
  point names.
- **Dimensions**: simple dimensions and dimension lines, separately.
- **Size**: the exported size is chosen at the top of the page setup window (and of the DXF and HPGL
  windows). File › “Export sizes…” writes one file per ticked size; a size nest is exported from “Size
  nest...” (SVG or PDF).

The **1:1 scale**, the calibration page and the assembly marks are never filtered. Page splitting is based
on what is **written**, so excluding a far element can shift the pattern on its pages. If your choice
leaves **nothing** to write, export is refused and no file is created. The preview is a list of what will
be written, **not** a paper proof.

### Export pieces as AAMA / ASTM-type DXF

File › **Export pieces as AAMA / ASTM-type DXF…**. This is an **AAMA / ASTM D6673-type** export: **opening
it in industry software has not been verified**, and the layer table still has to be confirmed against the
official specification. Try it in your software before relying on it.

- **Written**: all pieces of the project, one size, in **millimetres, 1:1**. One piece = one block, placed
  as in the pieces plan. Closed outline (curves split into segments within 0.001 mm), notches, grainline,
  drill holes and markings, buttons and buttonholes, internal elements and a text (name, code, quantity,
  material, pair).
- **Layers**: 1 piece outline · 4 notches · 6 grainline · 7 internal lines · 8 internal cutouts · 13 drill
  holes, marks and buttons · 15 sewing line · 84 text. Layers 2, 3 and 10 are declared but **left empty**.
- **Cut outline**: for a piece with a seam allowance, the drawn outline is the sewing line (layer 15) and
  the computed cut outline goes to layer 1. Without a seam allowance the outline is written **as drawn**.
- **No grading rule file (`.rul`)**.
- **Report before writing**: number of pieces, layers and entities, then **what is not exported** and
  remarks. If no piece can be written, no file is created. The project is never modified.

### Import a DXF and export construction geometry as DXF

**Import a DXF…** and **Export construction geometry as DXF…** (bottom of the File menu) are **generic**
exchanges of construction geometry, separate from the piece export above. **Import has only been tried on
hand-written test files and on PatternForge's own export, not on files from real software.**

**Import a DXF…** (text DXF only; binary DXF is refused):

1. Choose the file. Nothing is modified yet.
2. **Unit**: a recognised unit declared by the file is proposed; otherwise **you must choose** (mm, cm, m,
   inch or foot). Without a unit there is no preview and import stays disabled.
3. **Preview before import**: size in mm, factor, number of objects, imported entities by type, **ignored
   entities with their count and reason**, and the layer mapping.
4. Choose the **target sheet**, then **Import into the sheet**.

Lines, polylines (arcs kept), arcs, circles and points become **ordinary** paths and points: no piece or
constraint is created. A control-point `SPLINE` is **converted to segments** (within 0.005 mm); a
fit-point `SPLINE` becomes a smooth curve through those points. Z coordinates are ignored. Each DXF layer
becomes a layer of the same name. **Blocks are not exploded**: a block-based piece DXF imports empty, and
the report says so. The whole import is **one step**. A truncated, unreadable or oversized file (over
50 MB or 200,000 entities) is refused.

**Export construction geometry as DXF…** writes the paths of the current sheet (or all sheets) as DXF R12
text, in **millimetres, 1:1**: `LINE`, `ARC`, `CIRCLE`, `POINT`, curves as polylines (within 0.005 mm).
One layer per layer; a path whose role is not construction goes to `<layer>.<role>`. Notches, dimensions,
guides and groups are **not written** and the report says so.

### Export for a plotter (HPGL)

File › **Export for plotter (HPGL)…** writes the current sheet as an **HPGL-type** text file (`.plt`) at
**1:1** (1 unit = 0.025 mm). **It has not been tried on a real plotter or cutting software**: check the
scale on a first control sheet.

- Content and format follow “Output content” and “PDF page setup”. On a fixed sheet size the drawing must
  fit the printable area; otherwise export is refused and suggests a roll or a larger format. The drawing
  is **never rotated or shrunk**. The active X / Y correction factors are applied.
- **Pens**: 1 cutting outline · 2 sewing line · 3 construction · 4 grainline · 5 notches · 6 markings,
  buttons and buttonholes · 7 dimension lines and label text · 8 fold, special lines and axes · 9 dart
  legs. The file contains no pen-change pause.
- **Plot order**: by pen, then nearest neighbour from the bottom-left corner.
- **Not plotted**: dimension texts, point names and point markers (the count is shown).

### Advanced selection

`Edit › Selection`:

- **Select all** (`Ctrl+A`) and **Invert selection** (`Ctrl+I`): visible, unlocked objects of the active
  sheet only.
- **Lasso selection** (`Ctrl+L`): drag on empty space to draw a free area; an object is taken if it is
  **entirely** inside. `Shift` adds; holding `Alt` on release also takes crossed objects.
- **By role, layer or type** (`Shift`+click adds).
- **Select chain** (`Ctrl+Shift+A`, or double-click a path): all end-to-end paths. If the chain branches,
  nothing is selected and a message explains why.
- **Selection filters**: untick points, paths, notches, dimensions or guides to make them unreachable.

These operations do not modify the project.

### Direct input

While a tool expects values, a **bubble follows the cursor** with the same fields as the precise drawing
palette. An empty field shows the **observed measurement** (italic, marked “measured”); a typed value is
highlighted and marked “typed”. `Tab` moves between fields, `Enter` confirms, `Esc` cancels.

| Input | Meaning |
|---|---|
| `20;10` | **absolute** position x = 20 mm, y = 10 mm (Y upwards) |
| `@30;-5` | **relative** to the last point: dx = 30, dy = −5 |
| `@30<45` | **relative polar**: 30 mm at 45° (counterclockwise, 0° = to the right) |

With a decimal comma (French) separate x and y with `;`. In English `,` also works for positions
(`12.5,3`). A coordinate is a **creation parameter**: it is not kept as a constraint.

**Angle lock**: holding `Shift` while drawing a segment snaps the direction to the nearest multiple of
15°. A typed angle takes precedence. The lock takes precedence over snapping to an existing point.

**Repeat last command**: `Enter` with the Select tool, or the Edit menu. It **reactivates the last tool**
with its mode and last typed values; nothing is replayed automatically.

### Extract a piece

**Extract a piece** tool (`Z`, or Edit › Extract a piece from a closed area). Click **inside a closed
area**: the smallest region containing the click becomes a piece. The bordering paths need not meet at a
shared point. A shape entirely inside the region without touching its edge is not a hole. Ends less than
0.001 mm apart are considered joined.

- **Linked piece (default)**: the outline is made of **real paths** of the sheet, **split at crossings**
  where only partly used. Shapes and total lengths do not change. The piece **follows the construction**:
  moving an outline point recomputes area and perimeter. One undo step undoes the splits and the piece.
- **Detached** (toolbar box): the outline is **copied to a new sheet** named after the piece.
- **Area, perimeter and direction** are **observed measurements**, never saved.
- **Open area**: if a small gap prevents closure, the message gives the gap in mm and both ends are
  marked; nothing is created.
- **From a selection**: select paths forming exactly one closed outline, then Edit › “Create a piece from
  the selection”.

Limits: no hover preview; overlapping circles, arcs or curves are refused; a region whose outline touches
itself at a point is refused.

### Piece card

The sheet inspector (nothing selected) shows **one card per piece**: **Name**, **Code**, **Quantity**,
**Pair (×2 mirrored)**, **Material**, **Category** (fabric, lining, interfacing, leather, reinforcement)
and **Comment**.

- A field is applied when you leave it or press Enter; each is **one undo step**. A refused value shows a
  message and the previous value returns.
- **Warnings**: missing grainline, missing, open or self-crossing outline, deleted grainline, duplicate
  code. They are **notices**: the piece can still be saved.
- **Assign a grainline**: draw it with the Grainline tool (`G`), select it, then “Assign the selected
  grainline” in the piece card.

Limits: category names and vocabulary have not been reviewed by a pattern maker; `Ctrl+F` does not find
pieces; copying paths does not copy the piece; the card is written to the **piece label** of exports only
once it holds more than its name.

### Internal elements

Besides its outline, a piece can carry **internal elements**: **lines** (fold, topstitching, pocket
position, other) and **marks** (mark, buttonhole, other). They are **links** to construction paths, points
or notches, not copies. Section **Internal elements** of each piece:

- **Import from construction**: adds the paths, free points and notches **entirely inside the outline**,
  with a report of what was added, **ignored and why**. Running it again never duplicates.
- **Add the selection as internal element**.
- Each line shows name, length or position, a **type list** and **Remove**.
- Internal elements are drawn **on top** of the construction, with a line pattern per type and their name.
- Deleting a path, point or notch used as an internal element removes it from the piece in the same step.

Limits: types not reviewed by a pattern maker; this drawing has not been checked by eye.

### Pieces plan

**View › Pieces plan** shows **all pieces of the project laid flat**. It is a **layout**: moving or
rotating a piece there **never modifies the construction**.

- **Move**: drag a piece (one undo step; `Esc` or right-click cancels).
- **Rotate**: **Rotation (°)** field or **−90° −15° +15° +90°** buttons; **Center X / Y** place the center
  of the piece's bounding box.
- **Automatic placement**: until you move a piece, the plan arranges it and **nothing is saved**. “Reset
  to automatic placement” removes a manual placement.
- **Pieces list**: name, code, category, quantity, area, perimeter, warnings; click a header to sort, a
  row to frame the piece. **Fit all** frames the plan.
- **Overlap**: overlapping **bounding boxes** are flagged. This is **not exact** overlap detection.
- Sheet tools are inactive in the plan; **Back to the sheet** returns.

The pieces plan is a layout aid. For laying out on fabric or hides, use **File › Cutting plan…**.

### Derived pieces

A **derived piece** (facing, lining, interfacing or reinforcement) is created **from a parent piece** by
**offsetting its outline**, and stays **linked**: when you change the parent, it is **recomputed** in the
same undo step.

- **Create**: under the parent, **Create a derived piece…**: type, **offset** in mm (**positive =
  outwards, negative = inwards**), optional name. The derived piece gets its **own sheet**.
- **Out of date**: if recomputation is impossible, your change to the parent is **not refused**; the
  derived piece keeps its **last valid geometry** and says it is out of date.
- Its geometry **cannot be edited directly**; **Break the link** makes it an ordinary piece. Deleting the
  parent also breaks the link. Copying a sheet never copies the link. Up to 8 levels.

Limits: vocabulary not reviewed; offset only (mitred corners); no facing band bounded by a cut line; no
seam allowance.

### Split and join pieces

**Split** (Edit › **Split a piece along a line**, or **Split…** on the card): click two points of the cut
line, which is an **infinite line**. A preview shows the crossings and the **area of each half**.

- The piece is **replaced** by two **detached pieces** on their own sheets; the construction is **never
  modified**. One undo step. The areas add up to the original.
- Grainline and internal elements go to the half that **fully contains** them. Notches are **not
  carried**.
- Refused: a line that does not cross the outline, or crosses it at **more than two points**; a linked
  derived piece; an open or self-crossing outline.

**Join** (Edit › **Join two pieces…**, or **Join…** on the card): choose two pieces, **one edge of each**
and a **tolerance** (0.5 mm by default). The second piece is **moved without deformation** so that the
edges coincide. Length and shape gaps are **always shown**; outside tolerance, or if the pieces would
overlap, joining is refused. The result is one piece “A + B” on its own sheet.

Limits: two parts only; seam allowances are not recomputed for you; edges are chosen from a list;
**gestures and rendering have only been exercised by automated tests**.

### Unfold a piece built on a fold

A **half** drawn against its fold (a **straight segment of the outline**) becomes a **full symmetric
piece**: Edit › **Unfold a piece on its fold…**, or **Unfold…** on the card.

- A **new piece** on its own sheet: the half followed by its **mirror image**, with twice the area.
  Quantity, material and category are kept.
- It stays **linked to the half** and is recomputed with it; **Break the link** makes it ordinary.
- Refused: an outline crossing the fold line, a fold that is not a straight outline segment, an open or
  self-crossing outline.

Limits: grainline, internal elements and notches of the half are **not carried** to the unfolded piece;
one straight fold at a time.

### Drill holes and punch marks

A piece can carry **inner markings**, exported as a **separate group** and told apart by **shape**: a
**punch mark** (small filled 1 mm disc), a **drill hole** (circle with the diameter you give, 0.1 to
100 mm) or a **cross** (side 0.5 to 100 mm). Section **Drill holes and punch marks**:

- **Add a marking…**: shape and size, then **select a point** and **Add the marking**. The marking
  **follows the point**.
- **Set-back dart tip**: enter the **set-back** and select **two points** (the dart tip, then a point
  giving its direction). It is **not a dart**: only the marking position is computed.
- Each row has a size or set-back field and **Remove the marking**.
- Deleting the point removes the marking in the same step. A marking whose center leaves the piece is
  flagged. Split and Join carry markings; Unfold does **not**.
- **Export**: same shapes in SVG and PDF at 1:1 (checked in the file by tests, **not measured on paper**).

### Buttons and buttonholes

A piece can carry **groups of buttons or buttonholes** spread **evenly between two points**; positions are
**computed** and follow the points.

- **Add buttons…**: buttons or buttonholes, **count** (1 to 100), **diameter**, or **length** and
  **width**, **orientation** of a buttonhole (horizontal, vertical, perpendicular to the axis, free
  angle) and **edge set-back**. Then select two points and **Add the group**.
- **Edge set-back** is the minimum wanted distance **between the edge of the element and the piece
  outline**. It is a **check**: closer elements are flagged, **nothing is refused or moved**.
- Each row can be edited; **Points = selection** replaces the two points; **Remove the group**.
- Split carries the groups whose two points are in the same half; Join carries them; Unfold does **not**.

Limits: one element type per group, even spacing only, no overlap check, no eyelets.

### Notch linked to a point

By default a notch sits at a **fixed distance** from its path's start. You can **link** it: select the
notch, then choose in the inspector's **ANCHOR** section (one click = one step):

- **Point**: at the **foot of the perpendicular** from a point onto the path.
- **Midpoint**: middle of the path, in **developed length**.
- **Proportion**: a percentage of the path length.
- **Carried**: a point is projected onto a **source path**; the distance from that path's start is carried
  onto the notch's path (from its start or its end). Same sheet only.
- **Fixed distance**: unlinks; the notch stays where it is.

Deleting the point or source path removes the notch (undoable); splitting, merging, shortening or
extending the path unlinks it. Split, join, unfold and derived pieces do not carry notches.

### Advanced grainline

Once a piece has a grainline, the **ADVANCED GRAINLINE** section sets how it reads; nothing is set by
default.

- **Direction**: **Drawn path**; **Parallel to…** or **Perpendicular to…** another path (a curve gives its
  chord); **Bias +45°** or **−45°** from the sheet's X axis. Computed on read, so the arrow follows the
  reference. “Reverse the arrow” flips it.
- **Fabric direction**: **None** (two open heads), **Nap** (one closed head) or **Pattern** (doubled
  heads). Readable without colour.
- **Rotation tolerance**: 0 to 180°. In the pieces plan, exceeding it shows a **warning**; nothing is
  refused.
- **Extra lines** (stripes, matching), up to **8**, each with its own settings.
- The computed arrow replaces the drawn path, which stays faintly visible and remains the selectable
  object.
- Split **freezes** the direction; Unfold does not carry the grainline.

Limits: bias limited to ±45°; the vocabulary and conventions have **not been reviewed by a pattern
maker**.

### Special lines and linked topstitching

**Special lines** (*center front, center back, waist, hip, bust, skiving, gluing, topstitching*) are
**path roles**, each with its own line pattern and width, **visible in black and white**. The software
derives no measurement or check from them.

**Linked topstitching**: a line **generated at a given distance from an edge** and **recomputed when the
edge changes**.

- **Create**: select the edge; in **LINKED TOPSTITCHING** type the **distance** (0.1 to 1000 mm) and
  create it **Inside / Outside** (closed edge) or **Left / Right** (open edge). For a whole piece, use
  “Topstitching along the outline” on its card.
- It follows the edge within the same undo step. If recomputation is impossible, your move is **not
  refused** and the line is marked **out of date**.
- A linked line cannot be edited directly; **Break the link** frees it.

Limits: this is a layout offset, **not a seam allowance**; one link per row; no stitch length (see
stitching lines for hole spacing).

### Dimension lines (length, angle, radius, projections)

A dimension is a **measurement**: recomputed from the construction, it **enforces nothing**.

- **Create**: select the objects, then a button in **DIMENSION LINES (MEASURES)**:
  - **Path length**: **developed length**, curves included; the line is parallel to the chord;
  - **Angle**: three points (**vertex first**) or two touching segments, measured **counterclockwise**
    from the first side;
  - **Radius** and **Diameter**: arc or circle;
  - **Horizontal** and **Vertical**: two points.
- Values are shown to the hundredth of a millimetre, with the text oriented along the line.
- **Label** (40 characters) and text shift, line offset, arc radius or leader direction can be set.
- A dimension whose object is deleted is **removed with a notice**; one that cannot be computed is
  neither drawn nor exported, **without refusing your move**.

Limits: the PDF embeds a subset of Noto Sans (SIL OFL): French accents, extended Latin and Greek are
faithful; characters missing from the font (Cyrillic, `≤ ≥ → ∞`, Japanese) become **“?”** in the PDF (they
stay intact in SVG). Dimensions are not selected with the mouse; no chained or area dimensions, no
tolerance.

### Assembly checks (seams, piece walk, notches, joins, report)

Everything below **measures**: nothing is enforced, and none of these measurements has been validated by a
pattern maker. **Edit › Seams…**

- **Paired seams**: declare that edge A of one piece is sewn to edge B of another. A is the side **to be
  eased**. The table gives **sewing** lengths (never cutting), the gap, measured and expected ease, and
  whether it is **within tolerance** (1 mm by default).
- **Piece walk**: “Carry notches over” copies notches to the other side at the same sewing distance, ease
  spread proportionally. They are real notches.
- **Notch check**: **missing** and **shifted** notches; “Carry over the missing ones” adds them.
- **Joins**: at each seam end, the angle of each piece, their sum (smooth at 180°) and the difference.
- **Control report**: **File › Export control report…** writes a **PDF** or a **CSV** gathering pieces,
  seams, notch checks, joins and anomalies. No date or time: the same project gives the same file.

### Views, units and grid

**View › Views, units and grid...**. The **display unit** (millimetres, centimetres or fractional inches,
1/2 to 1/64) only changes display and input: the project and the exports stay in millimetres. In inches a
rounded value is preceded by “≈”; type `3/8`, `1 1/16` or `1-1/16`; a written unit (`12 mm`) wins. All
length fields follow the unit. The **grid** is configurable and **rulers** follow the unit. **View › Zoom
to selection** (`End`) frames the selection. A **named view** keeps framing, zoom, sheet and visible
layers: “Save current view”, then “Restore”. These settings are stored outside the project.

### Photo or scan of a paper pattern: calibrated background and tracing

**View › Background image (photo, scan)…**. **Save the project first**: the image is **copied** into a
folder next to the project file (`skirt.assets/`); it is never written into the project or exported.
Formats: PNG and JPEG.

1. **Import** the photo or scan.
2. **Calibrate** on the preview: *two points* whose real distance you know, or *four points* (the corners
   of a known rectangle, bottom-left, bottom-right, top-right, top-left) to correct perspective.
3. **Place the background**: the image appears under the grid and paths at model scale. The window shows
   the calibration **residual**; with 2 or 4 points it is about 0 and does **not** measure your clicking
   error: check a known dimension with the Measure tool.
4. Set **opacity**, hide or **lock** the background; recalibrate, relink or remove it. A missing or
   modified image does not prevent opening the project.

**Tracing.** “Choose the Trace tool”, then click points along the outline. *Snap to image edges* helps aim
(“Compute edges”). A placed point is an ordinary point. “Apply smoothed (curves)” creates Bézier curves,
“Apply as segments” creates segments, in **one undoable step**.

Limits: JPEG EXIF orientation is not applied; never tried with a real photo of a paper pattern.

**Save as.** To another folder, images are **copied** next to the new file under the same folder name:
keep it with the file. The original is never modified; a conflicting or failed copy is named in a message.
Named versions are not copied.

### Cutting plan on a hide (leather)

In **File › Cutting plan…**, the **Hide (leather)** section replaces the rectangular fabric width with the
real outline of a hide: the **outline**, **flaws** to avoid (hole, scar, stain, thin area, other) and
**quality zones** (grade 1 = best, to 3), all polygons in millimetres.

- **Enter a polygon** by clicking vertices (or typing X and Y), from a closed polyline of the sheet, or
  from a piece outline. Each gesture is undoable; invalid polygons are refused.
- **Checks**: a piece **touching a flaw** (overlap area), **leaving the hide** (area outside), and the
  **worst grade** under each piece.
- **Required quality**: pieces not meeting it are listed (this setting is not saved).
- **Yield**: placed area over usable area.
- **Automatic placement** stays inside the outline, avoids flaws and respects the required quality. It is
  neither optimal nor guaranteed minimal.
- **Exports** (SVG, PDF 1:1, CSV) draw flaws and zones.

Limits: never tried with a real hide; two overlapping zones of the same grade count twice.

## 9. Shortcuts and theme

`F1` opens the shortcut table. **Tool** letters can be changed (“Change”, then a letter A–Z; a letter in
use is refused). `Ctrl+…` shortcuts and special keys are fixed; on macOS, `Ctrl` in this guide and in the
menus means the Cmd key. The choice is stored outside the project, like the language (View › Language) and
theme (View › Theme, light or dark).

Settings: `%APPDATA%\PatternCraft\settings.json` on Windows; `$XDG_CONFIG_HOME/patterncraft`, or
`~/.config/patterncraft`, elsewhere. Deleting the file restores the defaults.

## 10. MCP server (AI assistants)

`crates/pf-mcp` exposes a project to an assistant through the MCP protocol, locally. By default it is
**read only** (lists, measurements, validation). With `--allow-write` it also offers `preview_command`
(simulation), `apply_command` (with the expected file revision; refused if the file changed) and
`undo_last`, with a journal next to the project. Details (in French):
[../crates/pf-mcp/README.md](../crates/pf-mcp/README.md).

### Validate a project file

`pf-validate` checks a `.pforge.json` file without opening it in the application and without modifying it:
`cargo run -q -p pf-core --bin pf-validate -- my-project.pforge.json` (add `--json` for machine-readable
output). It applies the same rules as opening in the application. Exit code 0 = valid, 1 = invalid (the
message says why), 2 = unreadable file or bad command line. The format is described (in French) in
[FORMAT_PFORGE_V6.md](FORMAT_PFORGE_V6.md), with a JSON schema in
[pforge-v6.schema.json](pforge-v6.schema.json); the schema alone does not replace this check.

## 11. What the current version does not do

- No general constraint solver: only maintained constructions, formulas and the imposed length of a curve.
- No grading rule file (`.rul`) in DXF export; the AAMA / ASTM-type layer table is not confirmed and no
  compatibility with industry software is claimed.
- Automatic piece placement is an aid, not industrial optimisation.
- No macros, no automatic image vectorisation, no digitising table support.
- No test on paper or with a pattern maker has been done yet: trade conventions remain to be validated.

## 12. Troubleshooting

| Symptom | Likely cause |
|---|---|
| “No cutting line” on export | No path with the cutting outline role: the export is marked as a construction drawing |
| A tool shortcut does nothing | A text field has the focus; click the canvas or press `Esc` |
| Notch refused | Distance outside the path, or path shortened below an existing notch |
| File not opened | Format version newer than the application, or missing reference: the message says which |
