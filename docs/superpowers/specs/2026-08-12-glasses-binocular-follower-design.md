# Glasses Binocular Sync

Date: 2026-08-12  
Status: Implemented (pixel clone)

## Architecture

- **Left eye**: sole SPA WebView — pairing, ping, list/chat, temple, voice
- **Right eye**: `EyeSurfaceMirror` (`PixelCopy` ~20 FPS into an `ImageView`)
- No second SPA, no UI/session mirror bridge, no `follower` / `isPrimaryEye` gating

## Retired

JS UI-mirror publish/subscribe, session-mirror dual-eye hydrate, secondary
language push, and SPA `follower` branches were removed after the pixel-clone
path became the only right-eye implementation.
