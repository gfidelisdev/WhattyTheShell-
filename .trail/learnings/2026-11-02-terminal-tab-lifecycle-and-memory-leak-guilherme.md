---
title: Terminal tab lifecycle and memory leak prevention
author: Guilherme
date: 2026-11-02
branch: main
id: l-16594797
scope: repo
status: active
src: agent
touches:
  - src/fonts.rs
  - src/main.rs
  - src/pty.rs
  - src/render/atlas.rs
  - src/render/dmabuf.rs
  - src/render/pipeline.rs
  - src/split.rs
  - src/term/grid.rs
  - src/term/mod.rs
  - src/widget.rs
---

Terminal tab closing must eagerly terminate the PTY and its process group (SIGHUP + SIGKILL) and clear TerminalView inner state. Otherwise, child processes, background pty-reader threads, wgpu textures, and glibc malloc arenas leak indefinitely. Additionally, signal closures connected to child-exited must take the emitted view parameter rather than capturing a strong clone to avoid reference cycles.
