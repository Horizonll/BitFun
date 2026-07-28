# Glasses AR Edge HUD UI

Date: 2026-07-24  
Updated: 2026-07-28

## Goal

Optical see-through glasses must keep the center of the view clear for walking.
`glasses-web` defaults to an edge HUD; the center chat panel opens only when the
user explicitly expands a conversation.

Align with RayNeo X-series 《设计规范》 (root `设计规范.md`): low information
density, FoV-preserving layout, ≥16px type, 30×20 safe margins, low APL, green
accent preferred over red/warm fills.

## Locked decisions

| Decision | Choice |
|---|---|
| Default chrome | `hud` — clear center, status on edges |
| Detail chrome | `focus` — semi-transparent center chat panel |
| Enter focus | Tap a session in the left rail (or create new) |
| Exit focus | Tap the same session again in the left rail → `hud` (keep session id) |
| Scope | `src/glasses-web` only; no Desktop/relay/mobile-web changes |
| Files | Reuse `VrShell`, `SessionListPage`, `ChatPage`; no orphan HUD components |
| DoF | 0 DoF HUD (fixed to head / screen) |

## Layout

- **Left**: recent sessions only (max 6), always visible, no scroll; name + icon only
- **Right**: connection / mode / workspace|assistant name / create / status card / devices / language / disconnect
- **Safe margins** (设计规范 §5.4): ≥ `30px` horizontal, ≥ `20px` (or `4vh`) vertical, plus device safe-area
- **Column gap**: `20px`; stack gap `10px`; rail width `min(176px, 17vw)` to keep center clear
- **Controls**: `44px` min-height, shared padding, `24px` icons; host flush to rail
- **Center (hud)**: transparent environment reserve
- **Center (focus)**: messages (no outer dialogue frame / no title) + sibling input frame
- **Pickers / menus**: centered floating panel inside safe insets
- **No side-rail collapse**

## Visual chrome

Borderless low-APL frosted glass: `rgba(255,255,255,0.08)` + `blur(8px)`.
Accent tint uses green channel (`rgba(61,220,151,0.14)`). No strokes, outlines,
gradients, shadows, or large white/black slabs. One radius `--vr-radius: 16px`;
circular affordances stay fully round. No press `transform`/`translate`/`scale`.

| Surface | Treatment |
|---|---|
| Left / right rail containers | Clear |
| Labels / health | White text ≥16px |
| Interactive cards / buttons / tool UI | Low-APL frost |
| Dialogue host | Clear — no outer frame / no title |
| Input frame | Low-APL frost panel |
| Reconnect banner | Text only (no frost) |
| Pairing card | Low-APL frost |
| Native QR scan | Text + ≥2px stroke aiming frame |

## Typography (设计规范 §5.2)

- Minimum body size: `16px` (`--vr-font-min`)
- Titles: `18px` (`--vr-font-title`)
- Line height: `1.45` for breathing room
- Prefer short status copy; hide secondary rail metadata (counts, timestamps, category labels)

## Pairing / scan / rescan

- Native QR: text-only + stroke reticle; margins ≥20/30
- Pairing page: frost card, Chinese default, retry + rescan
- Host bridge `BitFunGlassesHost.rescanQr()` returns to camera scanner
- Reconnect banner: text-only actions — 重新配对 / 重新扫码

## Non-goals

- Binocular clones, eye tracking, temple-gesture ownership in web
- Auto HUD switching by motion sensors
- Full-screen opaque slabs or high-APL persistent chrome (>13% APL target)
