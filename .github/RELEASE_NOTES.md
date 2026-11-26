**Pre-release of a prototype.** No pattern maker has validated PatternForge yet and no print has been
measured on paper: print the 100 mm calibration square and measure it before cutting any material.

## Install

| System | Download |
|---|---|
| **Windows 10 / 11** | `patternforge_*_x64-setup.exe` (per-user install) |
| **macOS** | `PatternForge_*_aarch64.dmg` (Apple Silicon) or `PatternForge_*_x64.dmg` (Intel) |
| **Linux** | `patternforge_*_amd64.deb` or `patternforge_*_x86_64.AppImage` |

Check your download against the `SHA256SUMS-*.txt` files.

**The installers are not code-signed.** On Windows, SmartScreen shows "Windows protected your PC":
*More info* → *Run anyway*. On macOS, the application is blocked on first launch: after copying it to
*Applications*, run `xattr -dr com.apple.quarantine /Applications/PatternForge.app`.

The application has been used by hand on **Windows only**. Linux and macOS builds are checked
automatically, but nobody has used them by hand yet: please report problems in the issues.

The interface starts in French; switch with *View › Language* (*Affichage › Langue*).
