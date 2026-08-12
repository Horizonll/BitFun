# Glasses Session List Minimal UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Sessions page = history list + bottom new-session only; remove language/disconnect chrome.

**Architecture:** Strip `VrShell` sessions chrome; simplify `SessionListPage` pageMode render/interactions; keep binocular mirror for page/session/focus.

**Tech Stack:** React SPA (`src/glasses-web`), sync to Android assets, installDebug.

### Task 1: VrShell strip chrome
- Remove header, language, disconnect, devices, disconnect dialog
- Drop unused props (`onDisconnect`, `onRescan`, `client` if unused)
- Clean UI mirror fields for disconnect/language if unused

### Task 2: SessionListPage minimal pageMode
- Flat history list only (no workspace tree chrome in pageMode)
- Create button below list
- Disable rename/delete/long-press/context menu
- Remove section title/count chrome

### Task 3: App wiring + styles
- Stop passing disconnect into VrShell
- Remove repair/disconnect affordance from reconnect banner (keep status / optional rescan)
- CSS: sticky bottom create button layout

### Task 4: Verify
- type-check, build, sync, installDebug on glasses
