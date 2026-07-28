# AGENTS.md

Glasses-web is the browser SPA bundled into the RayNeo X3 Android companion.

## Boundaries

- Keep logic inside `src/glasses-web`; do not import from `src/mobile-web` or `src/web-ui`.
- Pairing is auto-only for normal remote-control QR; no username/password form.
- Account-auth QR is unsupported on glasses (error + re-scan).
- Locale ids come from `src/shared/i18n/contract/locales.json` via generated files; do not import Web UI locale catalogs.

## Where to look first

| Area | Paths |
|---|---|
| Auto-pair | `src/pages/PairingPage.tsx` |
| VR shell | `src/pages/VrShell.tsx`, `src/App.tsx` |
| Relay / sessions | `src/services/` |
| Styles | `src/styles/components/vr-shell.scss` |

## Verification

```bash
pnpm --dir src/glasses-web run type-check
pnpm --dir src/glasses-web run build
```
