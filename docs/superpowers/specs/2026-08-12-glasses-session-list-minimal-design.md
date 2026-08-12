# Glasses Session List Minimal UI Design

Date: 2026-08-12  
Surface: `src/glasses-web` + Android assets sync only  
Status: Approved (Approach A)

## Goal

Simplify the paired session list page for RayNeo glasses:

- Remove language toggle UI
- Remove disconnect UI from the session shell
- Session list page shows only historical sessions + a bottom “New session” button
- Clicking a session opens chat only (no rename/delete)

## Non-goals

- Do not change Desktop, relay, or `src/mobile-web`
- Do not remove i18n runtime (keep default `zh-CN`)
- Do not remove native pairing / QR / voice / binocular UI mirror
- Chat page layout stays as-is

## Visible UI (sessions page)

```
┌─────────────────────┐
│  session row        │
│  session row        │
│  ...                │
│                     │
│  [ 新建会话 ]       │
└─────────────────────┘
```

Hidden / removed from this page:

- Shell header (title, connection health, devices, language, disconnect)
- Disconnect confirm dialog
- Devices page entry
- Assistant picker / workspace tree chrome beyond what is required to load sessions
- Session context menu, rename modal, delete confirm
- Section title / item count chrome if it adds clutter beyond the list

## Interaction

| Action | Result |
|--------|--------|
| Tap session | Open Chat for that session |
| Tap 新建会话 | Create code session (primary eye), then open Chat |
| Long-press / context menu | Disabled |

## Backend / sync

- Primary eye still owns mutations and UI mirror for page/session/focus
- Drop `showDisconnectConfirm` and `language` from UI mirror payload when unused (or leave fields unused but stop publishing language changes from UI)
- `handleDisconnect` may remain in `App` for host-driven teardown if needed; no sessions-page affordance

## Files

- `src/glasses-web/src/pages/VrShell.tsx` — strip header/actions/disconnect/devices
- `src/glasses-web/src/pages/SessionListPage.tsx` — list + create only; no menus
- `src/glasses-web/src/App.tsx` — stop passing disconnect into VrShell if unused
- `src/glasses-web/src/services/uiMirror.ts` — optional field cleanup
- styles under `src/glasses-web/src/styles` as needed

## Verification

1. `pnpm --dir src/glasses-web run type-check`
2. `pnpm --dir src/glasses-web run build` + sync assets + `installDebug`
3. Device: sessions page shows only list + new button; both eyes stay in sync; open/create session works
