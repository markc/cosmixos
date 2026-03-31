# Directory Opus + ARexx: Deep Dive & Cosmix Clone Architecture

## Screenshot Reference

The best publicly available screenshot collection of classic DOpus (Amiga) in action is at:

**https://datagubbe.se/dopus/**

This page by a dedicated Amiga enthusiast shows DOpus 4 in its classic dual-pane layout with the button bank, ARexx gadget, and file listers. It also shows the configurator screens for buttons, filetypes, and the hex viewer. The article itself is titled "Directory Opus — King of the Dual Panes" and is an excellent visual reference for the UI we'd want to channel.

Additional screenshots and the full DOpus 5.5 manual (scanned) are at:
- **https://archive.org/details/Directory_Opus_5.5_1996_GPSoftware**
- DOpus 5 open source: **https://sourceforge.net/projects/dopus5allamigas/**

---

## How DOpus Worked — The Architecture

### DOpus 4 (The Classic)

DOpus 4 was a single-window, dual-pane file manager with a configurable button bank running across the middle. It had:

- **Two file listers** (source and destination) — the core mental model
- **A configurable button bank** between or around the listers — each button could trigger AmigaDOS commands, ARexx scripts, or internal DOpus functions
- **A dedicated ARexx gadget** — button marked "A" that let you type and execute ARexx commands/scripts directly
- **Drag-and-drop** between panes (copy, move, unpack archives by dragging)
- **Built-in viewers** for IFF images, text, hex, MOD audio — no external programs needed
- **Filetype recognition** — custom actions per filetype on double-click

Every button could have different actions for left-click, right-click, and middle-click. The entire interface was configured through a visual editor — no config files to hand-edit.

### DOpus 5 / Magellan (The Revolution)

DOpus 5 was a ground-up rewrite by Jonathan Potter (Brisbane, Australia — GPSoftware). It transformed DOpus from a file manager into a full **Workbench replacement**. Key architectural changes:

- **Everything became an object** — Listers, Button Banks, Menus, Scripts, HotKeys, and Filetype actions were all instances of a unified "Opus Button Object" class
- **Fully multi-threaded** — each Lister, each file operation, each button bank ran as an independent Amiga task. You could copy files in one lister while browsing FTP in another while configuring buttons in a third
- **Three display modes per Lister**: Name Mode (detailed list), Icon Mode (like Workbench), and Icon Action Mode (icons with Name Mode power)
- **Lister toolbars** — each Lister could have its own toolbar with custom buttons
- **Custom popup menus** — right-click context menus were fully configurable per filetype
- **Scripts system** — event-driven scripts triggered by disk insertion, lister open/close, startup/shutdown
- **OpusFTP module** — browse remote FTP sites in standard Listers as if they were local directories
- **Loadable modules** — functionality extended via `.module` files (FTP, archive handling, etc.)

---

## How DOpus Used ARexx — The Message Port Architecture

This is the critical part for Cosmix. DOpus exposed an ARexx message port named `DOPUS.1` (or `DIRECTORYOPUS.1` in DOpus 5). Any ARexx script anywhere on the system could send commands to this port to control DOpus remotely.

### The Three Command Namespaces (DOpus 5)

The DOpus 5 ARexx API was organized into three object hierarchies, documented in Chapter 16 of the manual:

#### 1. `dopus` — Global Application Commands

These controlled the application itself:

| Command | Description |
|---------|-------------|
| `dopus front` | Bring DOpus to front |
| `dopus back` | Send DOpus to back |
| `dopus getfiletype <file>` | Identify a file's type |
| `dopus getstring <prompt>` | Show a string requester |
| `dopus request <text>` | Show a requester dialog |
| `dopus addappicon` | Add an application icon |
| `dopus remappicon` | Remove an application icon |
| `dopus addtrap` | Trap an internal command |
| `dopus remtrap` | Remove a command trap |
| `dopus send` | Send an internal command |
| `dopus set <option>` | Set environment options |
| `dopus query <option>` | Query environment state |
| `dopus set sound` | Set sound for an event |
| `dopus query sound` | Query sound for an event |
| `dopus screen` | Query screen information |
| `dopus version` | Get DOpus version |

#### 2. `lister` — Lister Window Commands

These controlled individual file listers, identified by a lister handle:

| Command | Description |
|---------|-------------|
| `lister new` | Open a new lister (returns handle) |
| `lister new <path>` | Open lister at path |
| `lister close <handle>` | Close a lister |
| `lister read <handle> <path>` | Read directory into lister |
| `lister refresh <handle>` | Refresh lister display |
| `lister query <handle> path` | Get current path |
| `lister query <handle> numfiles` | Get file count |
| `lister query <handle> numselfiles` | Get selected file count |
| `lister query <handle> firstsel` | Get first selected file |
| `lister query <handle> nextsel` | Get next selected file |
| `lister query <handle> selfiles` | Get all selected files (via stem) |
| `lister query <handle> entry <name>` | Get file details |
| `lister query <handle> source` | Is this the source lister? |
| `lister query <handle> dest` | Is this the destination lister? |
| `lister query <handle> header` | Get lister title |
| `lister query <handle> display` | Get display format |
| `lister query <handle> sort` | Get sort order |
| `lister query <handle> position` | Get window position |
| `lister query <handle> visible` | Is lister visible? |
| `lister query <handle> busy` | Is lister busy? |
| `lister set <handle> source` | Set as source lister |
| `lister set <handle> dest` | Set as destination lister |
| `lister set <handle> off` | Turn off source/dest |
| `lister set <handle> busy on` | Set lister to busy state |
| `lister set <handle> busy off` | Clear busy state |
| `lister set <handle> header <text>` | Set title bar text |
| `lister set <handle> display <format>` | Set display format |
| `lister set <handle> sort <field>` | Set sort field |
| `lister set <handle> position x/y/w/h` | Set window geometry |
| `lister set <handle> mode name` | Switch to name mode |
| `lister set <handle> mode icon` | Switch to icon mode |
| `lister set <handle> toolbar <name>` | Set lister toolbar |
| `lister set <handle> progress <info>` | Control progress bar |
| `lister add <handle> <entry>` | Add entry to lister |
| `lister remove <handle> <name>` | Remove entry from lister |
| `lister select <handle> <name>` | Select a file |
| `lister clear <handle>` | Clear all entries |
| `lister empty <handle>` | Empty the lister |
| `lister wait <handle>` | Wait for lister to finish |
| `lister iconify <handle>` | Iconify the lister |

#### 3. `command` — Internal Command Execution

This let you execute any internal DOpus command:

| Command | Description |
|---------|-------------|
| `command copy` | Copy selected files |
| `command move` | Move selected files |
| `command delete` | Delete selected files |
| `command rename` | Rename selected files |
| `command makedir` | Create directory |
| `command run` | Run a program |
| `command <any>` | Any internal Opus command |

### Custom Handlers — The Killer Feature

DOpus 5 introduced **Custom Handlers** — the ability for an ARexx script to take over a Lister's behavior entirely. This is architecturally profound and directly maps to what AMP could do for Cosmix.

When you set a custom handler on a lister, DOpus would send ARexx messages to your script whenever anything happened in that lister. The handler could then populate the lister with arbitrary data — it didn't have to be a filesystem directory.

**This is exactly how OpusFTP worked internally.** The FTP module registered a custom handler that intercepted all lister operations and translated them to FTP commands. To the user, the FTP site looked identical to a local directory. Internally, every file operation was being routed through ARexx to the FTP handler.

**Trapped functions** available to custom handlers included:

- `path` — user changed directory path
- `doubleclick` — user double-clicked a file
- `drop` — user dropped files onto the lister
- `dropfrom` — files were dragged FROM this lister
- `parent` — user clicked parent button
- `root` — user clicked root button
- `reread` — user requested directory refresh
- `inactive` — lister became inactive
- `active` — lister became active

Additionally, custom handlers could create **AddStem pop-ups** — custom right-click context menus populated dynamically by the handler script.

### Example: A Real DOpus ARexx Script

Here's what a typical DOpus ARexx interaction looked like:

```rexx
/* Open two listers side by side and copy selected files */
ADDRESS 'DOPUS.1'

/* Open source lister */
lister new 'SYS:'
srchandle = RESULT

/* Open destination lister */
lister new 'Work:Backup'
dsthandle = RESULT

/* Position them side by side */
lister set srchandle position 0/0/320/400
lister set dsthandle position 320/0/320/400

/* Set source and destination */
lister set srchandle source
lister set dsthandle dest

/* Wait for source to finish reading */
lister wait srchandle

/* Query selected files */
lister query srchandle numselfiles
IF RESULT > 0 THEN DO
    command copy
END
```

And a custom handler skeleton:

```rexx
/* Custom FTP-like handler for a lister */
ADDRESS 'DOPUS.1'

/* Create a new lister with custom handler */
lister new
handle = RESULT
lister set handle handler myport
lister set handle header 'My Custom Source'

/* Main message loop */
DO FOREVER
    CALL WAITPKT('myport')
    packet = GETPKT('myport')
    IF packet = '00000000'x THEN ITERATE

    command = GETARG(packet, 0)
    handle = GETARG(packet, 1)
    arg = GETARG(packet, 2)

    SELECT
        WHEN command = 'path' THEN DO
            /* User navigated to a path - populate lister */
            lister clear handle
            lister add handle '"MyFile.txt" 1024 -rwed 01-Jan-96 12:00'
            lister refresh handle
        END
        WHEN command = 'doubleclick' THEN DO
            /* User double-clicked a file */
            /* ... handle the action ... */
        END
        WHEN command = 'drop' THEN DO
            /* Files were dropped onto this lister */
            /* ... handle upload/transfer ... */
        END
        OTHERWISE NOP
    END

    CALL REPLY(packet, 0)
END
```

---

## Mapping DOpus → Cosmix Architecture

### The Direct Parallels

| DOpus Concept | Cosmix Equivalent | Notes |
|---|---|---|
| ARexx message port `DOPUS.1` | AMP port `cosmix-files.cachyos.amp` | DNS-style addressing |
| `lister new` / `lister close` | AMP `Lister.Open` / `Lister.Close` | Same object-handle pattern |
| `lister query <handle> <prop>` | AMP `Lister.Query` with handle + field | Direct mapping |
| `lister set <handle> <prop>` | AMP `Lister.Set` with handle + field + value | Direct mapping |
| `dopus getfiletype` | Content-type detection via `cosmix-indexd` | AI-enhanced |
| Custom Handlers | AMP Custom Handler registrations | The architecture is identical |
| OpusFTP module | JMAP handler, VPS handler, S3 handler, etc. | Each backend = custom handler |
| Button Bank objects | Widget toolbar with AMP-addressable buttons | `copy-btn.toolbar.cosmix-files.cachyos.amp` |
| Filetype system | Filetype manifests with Mix/Lua actions | AI-classified + rule-based |
| Scripts (event-driven) | AMP event subscriptions | `OnDiskInsert` → AMP event |
| Internal commands | Mix built-in file commands | `copy`, `move`, `delete`, etc. |
| `command <any>` | `ADDRESS 'cosmix-files'` in Mix | `send` keyword |

### The AMP Wire Format Advantage

DOpus ARexx used simple string commands over Amiga message ports. AMP's markdown-frontmatter + JSON format gives us the same human readability with structured data:

```
---
to: lister.cosmix-files.cachyos.amp
from: my-script.cachyos.amp
op: Query
id: req-001
---
{"handle": "lister-7f3a", "field": "selfiles"}
```

Response:
```
---
to: my-script.cachyos.amp
from: lister.cosmix-files.cachyos.amp
op: QueryResult
re: req-001
---
{"files": ["document.pdf", "photo.jpg", "notes.md"]}
```

### Custom Handlers in AMP — The Killer Feature Preserved

Just as DOpus's FTP module used custom handlers to make FTP sites appear as local directories in a Lister, Cosmix can use AMP custom handlers to make *anything* appear as a navigable file list:

- **JMAP mailboxes** — email folders as Listers, messages as entries
- **VPS instances** — containers/VMs as directories, remote files browsable
- **Git repositories** — branches as directories, commits as navigable history
- **S3/object storage** — buckets and prefixes as the directory hierarchy
- **Mesh nodes** — other Cosmix nodes' filesystems, transparently
- **Database tables** — rows as entries, columns as file metadata fields

The custom handler registers with AMP and receives the same events DOpus trapped: navigation, double-click, drop, drag-from, refresh. The Lister UI is completely agnostic to the backend — it just renders entries and sends events to whatever handler owns it.

### Mix Scripting — The ARexx Equivalent

Where DOpus users wrote ARexx scripts, Cosmix users will write Mix scripts:

```mix
# DOpus-style file management script in Mix
# Open two listers side by side and sync selected files

address cosmix-files

$src = send Lister.Open path="/home/mark/projects"
$dst = send Lister.Open path="/mnt/backup/projects"

send Lister.Set handle=$src role=source
send Lister.Set handle=$dst role=dest

$count = send Lister.Query handle=$src field=numselfiles

if $count > 0
    send Command.Copy source=$src dest=$dst
    say "Copied $count files to backup"
end
```

This preserves the ARexx mental model exactly: `ADDRESS` sets the target port, `send` dispatches commands, PARSE can destructure responses, stems can hold file lists.

---

## Cosmix DOpus Clone — Component Plan

### `cosmix-files` — The Application

A Dioxus desktop application providing:

1. **Lister widget** — the core dual-pane file display (Name Mode first, Icon Mode later)
2. **Button bank widget** — configurable toolbar with per-button AMP actions
3. **Path bar** — editable path with breadcrumb navigation
4. **Status bar** — file count, selection count, disk space
5. **Viewer pane** — preview panel for images, text, hex
6. **AMP port** — `cosmix-files.{node}.amp` accepting all Lister/Command operations

### AMP Port Commands (Initial Set)

```
Lister.Open          — open a new lister, returns handle
Lister.Close         — close a lister by handle
Lister.Read          — read a directory into a lister
Lister.Refresh       — refresh lister contents
Lister.Query         — query lister state (path, files, selection, etc.)
Lister.Set           — set lister state (source/dest, header, sort, etc.)
Lister.Add           — add an entry to a lister
Lister.Remove        — remove an entry
Lister.Select        — select/deselect entries
Lister.Clear         — clear all entries
Lister.Wait          — wait for async operation to complete
Lister.SetHandler    — register a custom handler for this lister

Command.Copy         — copy selected files source→dest
Command.Move         — move selected files
Command.Delete       — delete selected files
Command.Rename       — rename a file
Command.MakeDir      — create directory
Command.Run          — execute a file/program
Command.View         — open file in viewer pane

App.Front            — bring to front
App.GetFileType      — identify file type
App.Request          — show a dialog
App.Version          — query version info
```

### Custom Handler Protocol

When a Mix/Lua script registers as a custom handler for a Lister:

```mix
address cosmix-files

$lister = send Lister.Open
send Lister.SetHandler handle=$lister port=my-jmap-handler.cachyos.amp

# The lister will now send these events to my-jmap-handler:
#   Navigate   — user changed path
#   Activate   — lister gained focus
#   Select     — user selected entries
#   DoubleClick — user double-clicked entry
#   Drop       — files dropped onto lister
#   DragFrom   — files dragged from lister
#   Refresh    — user requested refresh
#   Close      — lister is closing
```

### UI Technology

- **Dioxus desktop** (WebKitGTK) for the initial implementation
- Lister widget renders via HTML/CSS in WebKitGTK — fast, flexible, styleable
- Future: native rendering via `cosmix-comp` Wayland compositor + `cosmix-shell`
- All UI elements AMP-addressable: `copy-btn.toolbar.cosmix-files.cachyos.amp`

---

## Why This Matters

DOpus wasn't just a file manager. It was a **platform**. Through ARexx, any program on the system could ask DOpus to open a lister, display files, respond to user actions. DOpus became the visual shell through which users interacted with everything — local files, FTP sites, archives, devices.

Cosmix `cosmix-files` with AMP custom handlers recreates this exact paradigm:
- Any Cosmix service can present data through the familiar Lister UI
- Mix scripts orchestrate file operations across backends transparently
- The user sees one consistent interface whether browsing local files, JMAP mailboxes, remote VPS filesystems, or mesh node storage
- Every UI element is scriptable and automatable through AMP

This is the ARexx dream, fully realized in Rust, on modern Linux.
