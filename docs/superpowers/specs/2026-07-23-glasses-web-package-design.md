# Glasses Web Package (RayNeo X3 Companion SPA)

Date: 2026-07-23

## Goal

Ship a **standalone** SPA for RayNeo X3 glasses remote control so:

1. Phone `src/mobile-web` stays unchanged for phone browsers (no VR branches).
2. Glasses get a VR split UI bundled inside the Android APK.
3. Day-to-day glasses iteration does not require rebuilding or re-uploading Desktop-hosted mobile-web.
4. After QR scan, pairing is **automatic** — no username / user-id / password form.

## Locked decisions

| Decision | Choice |
|---|---|
| Package strategy | **A + A1**: new `src/glasses-web`, **copy** needed pairing/session/chat services from mobile-web (no shared core extract in this phase) |
| Original mobile-web | **Revert all VR changes** (`VrShell`, `vrUi`, `layout="vr"`, App VR branch, vr-shell styles, AGENTS VR notes added for VR) |
| Desktop / relay | **No changes** |
| Import boundaries | Must not import `src/web-ui` or `src/mobile-web` |
| Pairing UX | Auto-pair with fixed identity; **no input form** |
| Pairing auth mode | MVP supports **normal remote-control QR only** (non-account). Account-auth QR → explicit unsupported error + re-scan, still no form |
| Android hosting | WebView navigates to scanned relay URL (same-origin `/api`); static assets served from APK via `shouldInterceptRequest` |
| UI mode flag | Glasses package is always VR layout; **`ui=vr` is not required** (remove Android/frontend dependency on that flag for the glasses path) |

## Non-goals

- Extracting a shared `mobile-remote-core` package (future optional)
- Account-auth pairing on glasses
- Peer Device Mode / HostInvoke
- Dual WebView binocular clones
- Temple gestures / focus trackers
- Changing Desktop remote_connect or relay protocols
- Keeping VR code inside `src/mobile-web` behind a flag

## Architecture

```text
Desktop shows Remote Connect QR (unchanged)
        │
        ▼
Android QrScanActivity (camera)
        │
        ▼
MobileWebActivity / GlassesWebActivity
  loadUrl(scanned pairing URL)   ← keeps relay origin for /api
  intercept GET static assets  ← APK assets/glasses-web/*
  /api and /ws                 ← network (relay)
        │
        ▼
src/glasses-web (bundled SPA)
  auto-pair (no form) → VrShell (sessions | chat)
```

Phone browsers continue to load Desktop-hosted `src/mobile-web` as today.

## Package layout

```text
src/glasses-web/
  package.json                 # name: bitfun-glasses-web
  vite.config.ts               # base: './', outDir: dist
  index.html
  tsconfig.json
  src/
    main.tsx
    App.tsx                    # pair → VrShell only (no phone stack nav)
    pages/
      PairingPage.tsx          # auto-pair UI (spinner / error only)
      VrShell.tsx
      SessionListPage.tsx      # VR layout only (copied + simplified)
      ChatPage.tsx             # VR layout only (copied + simplified)
      WorkspacePage.tsx        # optional overlay (copied as needed)
      DevicesPage.tsx          # optional overlay (copied as needed)
    services/                  # copied from mobile-web (relay, session, store, crypto…)
    hooks/
    i18n/                      # package-local messages (no web-ui locales)
    styles/                    # VR shell + minimal base styles
    utils/
```

Workspace:

- Add `src/glasses-web` to `pnpm-workspace.yaml`.
- Root scripts: `build:glasses-web`, `type-check:glasses-web` (and use them in Android sync docs).

Copy policy:

- Copy only files required for pair + poll + sessions + chat + VR overlays.
- Do not create a runtime dependency on `bitfun-mobile-web`.
- Drift is accepted until a later shared-core extraction; document the copy source in the package README.

## Pairing behavior (glasses)

On load with a valid pairing URL (`room` + `pk` present):

1. Resolve identity automatically:
   - Display / pair user id: fixed string `BitFun Glasses` (or product-stable equivalent).
   - Persist/reuse a local `installId` the same way mobile-web does for device identity.
2. Call the existing remote-control pair API path (copied client) **without** showing a form.
3. Show only connecting spinner / status / error.
4. On success → enter `VrShell`.
5. If the URL indicates **account-auth** pairing (`requiresAccountAuth` / account credentials required):
   - Do **not** show username or password fields.
   - Show a clear unsupported message and a path back to re-scan (Android finishes WebView or exposes re-scan).
6. Never gate the happy path on typing a username.

Phone `mobile-web` pairing forms remain unchanged.

## VR UI

- Left ~32%: workspace entry + session list (selected session highlighted).
- Right ~68%: chat for the selected session.
- Workspace / devices open as overlays when needed.
- No phone-style stack navigation in this package.

## Android integration

| Item | Behavior |
|---|---|
| Asset root | `app/src/main/assets/glasses-web/` (rename from prior `mobile-web` assets) |
| Sync script | `tools/sync-glasses-web-assets.ps1` (from `src/glasses-web/dist`) |
| Interceptor | Map relay document/static paths → `glasses-web/...`; never intercept `/api` or `/ws` |
| URL flags | Do not require appending `ui=vr` |
| Day-to-day build | `./gradlew :app:assembleDebug` / `installDebug` only |
| When UI source changes | `pnpm --dir src/glasses-web run build` then sync script, then Android build |

Native modules stay: QR scan, WebView host, pairing URL validation, Mercury init, phone preview scale.

## Revert scope (`src/mobile-web`)

Remove VR-only additions and restore phone-only App flow:

- Delete `src/pages/VrShell.tsx`, `src/utils/vrUi.ts`, `src/styles/components/vr-shell.scss`
- Revert `App.tsx`, `SessionListPage.tsx`, `ChatPage.tsx`, `styles/index.scss`, and VR notes in `AGENTS.md` if added solely for VR

Phone behavior after revert must match pre-VR mobile-web (stack navigation, pairing form intact).

## Verification

1. `pnpm --dir src/mobile-web run type-check` — still passes; no VR entrypoints.
2. `pnpm --dir src/glasses-web run type-check` and `build`.
3. Android unit tests for asset router + pairing URL helpers; `assembleDebug` / `installDebug`.
4. Manual glasses:
   - Scan normal Remote Connect QR → auto-pair (no username UI) → VR split shell.
   - Chat tracks Desktop without re-selecting session.
   - Account-auth QR → unsupported message, no form.
5. Manual phone: Desktop-hosted mobile-web pairing form + stack UI unchanged.
6. Confirm Desktop did not need a mobile-web rebuild/upload for glasses VR UI.

## Success criteria

- Glasses APK contains `glasses-web` assets and works against an older Desktop-hosted mobile-web.
- `src/mobile-web` has no glasses/VR presentation code.
- Users never type a username on the glasses happy path.
