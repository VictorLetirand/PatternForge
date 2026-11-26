# PatternForge

*Version française : [README.fr.md](README.fr.md).*

An offline 2D CAD workshop for pattern drafting (sewing and leatherwork), written in Rust. Pieces are
built in millimetres, with typed lengths and angles, then printed at 1:1 scale.

> **Status: prototype (0.1.0).** Nothing has been validated by a pattern maker yet, and no print has been
> measured on paper. Before cutting material, print the 100 mm calibration square and measure it. Trade
> conventions (seam allowances, notches, grading, layout) still have to be confirmed.

## What you can do

- **Draft precisely**: segment (length and angle), perpendicular, parallel, point at distance, rectangle,
  arc, circle, curves with developed length, snapping, dimensions.
- **Keep links, when you ask for them**: maintained constructions, variables and formulas, imposed curve
  length. Nothing follows automatically unless you choose it.
- **Work on pieces**: sheets, layers, piece extraction, seam allowances, notches, grainline, darts,
  pleats, drill holes, buttonholes, assembly checks.
- **Derive**: sizes and grading, variants, measurement tables and client records.
- **Leatherwork**: stitching lines, skiving and gluing zones, layout on a hide.
- **Lay out and output**: cutting plan, tiled A4 PDF at 1:1, SVG, DXF, HPGL, PNG and JPEG; direct printing
  and projection.
- **Reuse existing work**: DXF, SVG and vector PDF import, calibrated photo or scan to trace.
- **Automate**: command line ([docs/CLI.md](docs/CLI.md), in French) and a local MCP server, read-only by
  default ([crates/pf-mcp](crates/pf-mcp/README.md), in French).

The interface is available in French (default) and English (**View › Language**, or `--lang en`). Each
tool and its limits are described in the [user guide](docs/USER_GUIDE.md).

## Known limits

- No general constraint solver.
- No grading rule file (`.rul`); the AAMA / ASTM-type DXF layer table is not confirmed, and no
  compatibility with industry software is claimed.
- Automatic placement is an aid, not industrial optimisation.
- Developed and tried on Windows. Linux and macOS are targeted by CI, but the application has not been
  tried by hand there. Direct printing is Windows only. No installer, no signed binary.

## Build and run

A recent stable Rust toolchain is required (tried with 1.90).

```bash
cargo run --release -p pf-desktop
```

The executable is `target/release/patternforge` (`.exe` on Windows). It takes the path of a project to
open, `--demo` for a reference exercise and `--lang fr|en`. Example projects are in `examples/`.

On Linux, install the windowing and dialog libraries first (listed in
[.github/workflows/ci.yml](.github/workflows/ci.yml)).

## macOS

The application is not signed with Apple. A downloaded binary is therefore blocked on first launch: allow
it in System Settings › Privacy & Security, or remove the quarantine flag from the unpacked folder:

```bash
xattr -dr com.apple.quarantine PatternForge-macos-arm64
```

The archive built by CI targets Apple Silicon Macs; on an Intel Mac, build from source. Menus show “Ctrl”
where the shortcut uses the Cmd key.

## File format

A project is a versioned, documented `.pforge.json` text file:
[format version 6](docs/FORMAT_PFORGE_V6.md) (in French) and its
[JSON schema](docs/pforge-v6.schema.json). Earlier versions can still be read. To check a file:

```bash
cargo run -q -p pf-core --bin pf-validate -- my-project.pforge.json
```

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The integration tests of `pf-core` and `pf-export` form a single program per crate (`tests/suite.rs`): add
any new test file to its list.

| Folder | Role |
|---|---|
| `crates/pf-geometry` | Pure geometry, in mm |
| `crates/pf-core` | Model, editing commands, undo / redo, files |
| `crates/pf-export` | Exports and imports (PDF, SVG, DXF, HPGL, images) |
| `crates/pf-i18n` | French and English texts |
| `crates/pf-ui` | Shared interface elements |
| `crates/pf-cli` | Command line |
| `crates/pf-mcp` | Local MCP server |
| `apps/pf-desktop` | Application (egui) |
| `examples/` | Example projects and reference exports (compared byte by byte by the tests) |

Source comments and most technical documents are in French. Comments sometimes cite story or decision
numbers from internal working documents that are not published.

## Licence

[MIT](LICENSE). Third-party components and their licences: [THIRD-PARTY.md](THIRD-PARTY.md) (in French).
