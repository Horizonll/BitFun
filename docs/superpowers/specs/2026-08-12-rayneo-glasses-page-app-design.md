# RayNeo glasses page-app UI (BindingPair, ~1:1)

Date: 2026-08-12  
Status: approved (approach A)  
Reference: `VideoShowCase/glass` (`BaseMirrorActivity` + `BindingPair`, full-page per eye)

## Goal

Each eye shows the **same complete page** (not half of a widescreen HUD).
Viewport is ~**1:1**. Session list and chat are **separate pages**.

## Architecture

```text
MobileWebActivity : BaseMirrorActivity + BindingPair
  left WebView  ──┐ identical SPA URL / session mirror
  right WebView ──┘

SPA pages (stack):
  Pairing → SessionsPage → ChatPage
                 └──────→ DevicesPage
```

## Decisions

| Topic | Choice |
|---|---|
| Binocular | Keep BindingPair dual WebView; each eye full page |
| Aspect | Square-ish single column (~1:1), not wide dual-rail |
| Nav | Page stack; list and chat never side-by-side |
| Sessions page | Status, workspace/mode, list, create, devices, lang, disconnect |
| Chat page | Full page with back + composer + voice |
| Clear-view HUD | Removed (page app replaces edge HUD) |
| Temple focus | Keep `__bitfunTemple` + `[data-rayneo-focus]` |

## Out of scope

- Native Kotlin rewrite of chat (VideoShowCase FocusTracker only as reference)
- Desktop / relay / mobile-web changes
- Binocular QR scan
