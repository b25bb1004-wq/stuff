# Daily driver: Monolith as the operator's own tools and OS (decided 2026-11-16)

Operator, 2026-11-16: "everything u have built, i want to use it in place of the actual terminal, the actual GUI
or version control ... my own bootable OS ... everything u have built is going to be my own deployable and
usable stuff". Choices: daily tools on this PC first; the OS must boot THIS PC (x86-64, UEFI), not only QEMU.
Not the site: the website and app-UI polish are paused behind this track.

## Track A: daily tools on the PC (Windows 11 now, Linux/WSL too)
Rule: a tool is done when the operator uses it for a real day's work and it did not lose data or block him.
Each gap he hits becomes the next task. Order:
1. Krypton as the real shell: line editor (arrows, Home/End, Ctrl-A/E/W/U/R), persistent history, tab completion
   of paths and commands, runs real programs from PATH (Windows .exe/.cmd and Unix), pipes and redirection between
   real programs, env vars, `cd` with `~` and `-`, exit codes, Ctrl-C kills the child not the shell, a prompt with
   cwd and git/chronos branch, a startup file (~/.kryptonrc). Windows console raw mode via a small FFI shim
   (kernel32 SetConsoleMode/ReadConsoleInputW), each unsafe block with a SAFETY line.
2. Chronos as the real version control, GitHub-compatible: `chronos import` and `chronos export` through git's own
   fast-export/fast-import stream (spawn the git binary; no crates), so a chronos repo round-trips to GitHub with
   identical trees; then daily verbs (status, diff, add, commit, log, branch, switch, merge, stash) at git speed
   on this repo (strata).
3. Calliope as the terminal editor: open/save real files, undo, search, syntax colour for Rust/Logos/Markdown,
   mouse, large files (10 MB) without lag; `EDITOR=calliope` works with chronos commit.
4. Athena as the desktop IDE: the web app served locally by Mercury (`athena serve`), opening a real folder.
5. Mercury serving real sites (static + TLS later), Iridium as a real local database server, Argo installing
   Monolith's own tools into ~/.monolith/bin with one command, plus an installer script.
Packaging: one script installs every tool on Windows and Linux (see "Packaging" below). `argo install monolith` comes later.

## Packaging

Every Monolith command-line tool (27 binaries) is built in release mode and copied to `~/.monolith/bin`
(Windows: `%USERPROFILE%\.monolith\bin`). `VERSION` there holds the git short sha, and `MANIFEST` lists each
tool with its one-line `--help` summary. The tool list lives in `tools/install.sh` and `tools/install.ps1`.

Install (run from the strata folder; needs Rust/cargo):
- Windows 11: `powershell -ExecutionPolicy Bypass -File tools\install.ps1`
- Linux or Git Bash: `bash tools/install.sh`

Neither script changes PATH. Each prints the folder to add. To have it done for you, add `--add-to-path`
(sh: appends one guarded line to `~/.profile`) or `-AddToPath` (ps1: adds to the USER Path only, and refuses
if the entry is already there). Open a new terminal afterwards.

Update: pull, then run the same install command again. Tools are replaced in place. A tool that is running
is reported as NOT UPDATED; close it and run the command again.

Uninstall: `bash tools/install.sh --uninstall` or `powershell -ExecutionPolicy Bypass -File tools\install.ps1 -Uninstall`.
This removes `~/.monolith/bin`, `VERSION`, `MANIFEST` and the PATH entry the script added. Nothing else is touched.

Krypton as the Windows Terminal profile: the scripts do not edit Windows Terminal. In its settings (JSON),
add this to `profiles` > `list`, replacing `<you>` with your Windows user folder name:

```json
{
    "guid": "{fc1ef26d-1d99-4c66-bbbd-37a60ef7bef1}",
    "name": "Krypton",
    "commandline": "C:\\Users\\<you>\\.monolith\\bin\\krypton.exe",
    "startingDirectory": "C:\\Users\\<you>",
    "hidden": false
}
```

To make it the default, set `"defaultProfile": "{fc1ef26d-1d99-4c66-bbbd-37a60ef7bef1}"` at the top level of the settings.

## Track B: own OS booting this PC (x86-64 UEFI)
Titan today is RISC-V. Port plan, in order, each step gated in QEMU (OVMF) before real hardware:
1. UEFI boot stub (Rust target x86_64-unknown-uefi, built into rustc; needs `rustup target add`, ask first):
   prints to the GOP framebuffer, reads the memory map, exits boot services.
2. Kernel bring-up on x86-64: GDT/IDT, paging (4-level), APIC timer, serial, keyboard (PS/2 and USB HID later),
   then Titan's portable kernel core (scheduler from Kairos, allocator from Midas, files from Hades) on top.
3. Storage: NVMe or AHCI driver for a real disk partition with Hades on it.
4. Desktop: Iris on the GOP framebuffer, Krypton as the shell, Calliope as the editor.
5. Real hardware: boot from a USB stick on this PC (Secure Boot off; the operator does BIOS settings himself).
Risk: real-PC drivers (USB keyboard, NVMe, GPU modes) are the bulk of the work; GPU acceleration is out of scope.
