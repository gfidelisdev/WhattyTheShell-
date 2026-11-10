---
title: Buffer-anchored terminal text selection and drag auto-scroll
author: Guilherme
date: 2026-11-10
branch: main
id: l-56cbc1b3
scope: repo
status: active
src: agent
touches:
  - src/render/pipeline.rs
  - src/term/grid.rs
  - src/term/mod.rs
  - src/widget.rs
---

Selection was previously stored in viewport rows (0..rows), which caused the highlighted range to stay positional on screen when scrolling instead of following the scrolled text, and prevented selecting across scrollback. Storing selection in buffer coordinates ((scrollback.len() + viewport_row) - display_offset) keeps selection anchored textually across scrolling. Auto-scroll during mouse drag uses a GLib timeout timer (35ms) when the cursor enters the top/bottom threshold, updating display_offset and expanding selection until released.
