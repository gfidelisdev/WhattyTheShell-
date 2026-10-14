# Terminal Preferences & Theming

Manages user-configurable terminal settings, typographic choices, and color theming for the terminal emulator.

## Language

**Font Family**:
The name of a typographic face chosen for text rendering in the terminal.
_Avoid_: Font name, typeface identifier

**Monospace Font**:
A fixed-width font where every character occupies the identical horizontal advance width, preserving the grid matrix of the terminal screen.
_Avoid_: Proportional font, variable-pitch font

**Theme Palette**:
The complete set of background, foreground, cursor, and ANSI terminal colors that define the visual styling of the terminal emulator.
_Avoid_: Color scheme, skin

**Theme Swatch**:
A compact row or block of key colors providing an instant visual signature of a theme palette.
_Avoid_: Color bar, color sample

**Theme Preview**:
A simulated visual representation of terminal text and colors displayed to the user prior to applying a theme palette.
_Avoid_: Theme mockup, test run

**Built-in Theme**:
A curated theme palette embedded within the application for immediate, offline availability.
_Avoid_: Default theme, local preset

**Marketplace Theme**:
A theme palette retrieved dynamically from an external extension registry.
_Avoid_: Remote theme, online extension

## Packaging & Distribution

Manages build artifacts, distribution packages, system menu integration, and release workflows.

**Command Alias**:
A secondary symbolic link in the system execution path pointing to the primary application binary to provide alternative invocation names.
_Avoid_: Symlink shortcut, execution wrapper

**Desktop Entry**:
A Freedesktop-compliant file registering the application name, executable, search keywords, and icon within desktop environment menus.
_Avoid_: Shortcut file, launcher script

**Distributable Package**:
A versioned, pre-compiled package format (DEB, RPM, AppImage, or compressed archive) structured for distribution and deployment on target systems.
_Avoid_: Build output, raw binary

**Tarball Installer**:
A shell script bundled with a compressed archive that detects system or user privileges and deploys application binaries, aliases, icons, and desktop entries.
_Avoid_: Setup wizard, installer executable

## Text Interaction & Ergonomics

Manages grid text selection, clipboard synchronization, hyperlink activation, and window transparency controls.

**Text Selection**:
The continuous sequence of terminal cells marked by mouse drag, word double-click, or line triple-click for clipboard copying.
_Avoid_: Highlighted text, marked block

**Primary Selection**:
The system selection buffer populated immediately upon text selection, pasted via middle mouse click without modifier keys.
_Avoid_: Auto-clipboard, mouse buffer

**Bracketed Paste**:
A terminal protocol sequence enclosing pasted text within escape codes to safeguard against unintended command execution in interactive shells.
_Avoid_: Safe paste, raw paste

**Hyperlink Activation**:
The detection and opening of URLs in the default system browser triggered by modifier-click on recognized terminal text.
_Avoid_: Link parsing, web click

**Window Opacity**:
The visual transparency level of the terminal window, adjustable via modifier-scroll gesture or settings preferences down to a 20% floor.
_Avoid_: Alpha level, window see-through

**Color Supersaturation**:
An experimental rendering enhancement boosting color saturation and vibrance beyond standard limits when values exceed 100%.
_Avoid_: Color overdrive, brightness boost

**Tab Renaming**:
The custom alphanumeric label assigned by the user to identify an individual terminal tab session.
_Avoid_: Tab title change, header edit

**Tab Duplication**:
The creation of a new terminal tab session that inherits the exact working directory of an existing tab.
_Avoid_: Tab clone, replicate tab

