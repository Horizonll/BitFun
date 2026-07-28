# BitFun Glasses Web

Standalone SPA for the RayNeo X3 Android companion. Copied from
`src/mobile-web` (pairing / relay / session / chat services) and adapted for
VR split UI with **automatic pairing** (no username form).

Phone browsers continue to use `src/mobile-web`. This package must not import
`src/mobile-web` or `src/web-ui`.

## Commands

```bash
pnpm --dir src/glasses-web run type-check
pnpm --dir src/glasses-web run build
```

Sync into the Android APK after UI changes:

```bash
pwsh src/apps/mobile/android/tools/sync-glasses-web-assets.ps1
```

## Copy source

Service and page logic originated from `src/mobile-web` at the time this package
was created. Drift is expected until a shared core is extracted.
