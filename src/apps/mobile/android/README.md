# BitFun Android (RayNeo X3 Pro Companion)

Android APK for RayNeo X3 Pro glasses. Loads the standalone `src/glasses-web`
SPA (auto-pair + VR shell) from APK assets, while keeping the scanned relay URL
for same-origin `/api` calls.

## Scope (MVP)

- **Scan** the Desktop remote-control QR (camera)
- Open WebView against the scanned relay URL; serve HTML/JS/CSS from
  `app/src/main/assets/glasses-web`
- Auto-pair as `BitFun Glasses` (no username form) → VR split UI
- MercurySDK init + RayNeo app metadata for packaging

Explicitly **not** in MVP:

- Temple gestures / focus trackers
- Peer Device Mode (`HostInvoke`)
- Dual WebView binocular clones
- Desktop / relay code changes
- Account-auth pairing on glasses
- Kotlin chat / poll replica

## Flow

```text
QrScanActivity  →  MobileWebActivity (WebView)
     scan QR           navigate to pairing URL
                       intercept document/static assets from APK
                       /api stays on the network (relay)
                       glasses-web owns auto-pair + VR UI
```

## Build (Android only for day-to-day)

Requirements: JDK 17+, Android SDK (`compileSdk` 35 / `targetSdk` 34). Create
`local.properties` with `sdk.dir=...`.

```bash
cd src/apps/mobile/android
./gradlew :app:testDebugUnitTest :app:assembleDebug
./gradlew :app:installDebug
```

APK: `app/build/outputs/apk/debug/app-debug.apk` (debug id `com.bitfun.glasses.debug`).

### When to refresh bundled glasses-web

Only when `src/glasses-web` source changes:

```bash
pnpm --dir src/glasses-web run build
pwsh src/apps/mobile/android/tools/sync-glasses-web-assets.ps1
cd src/apps/mobile/android && ./gradlew :app:assembleDebug
```

## Phone debug (transparent AR preview)

Debug APKs using `mercury-stub` treat the phone as a small window onto the
glasses AR canvas: WebView lays out at ~1.8× phone size, then scales down
(`PhonePreviewScale.FACTOR = 0.55`). A **live rear-camera** preview sits under
the transparent WebView so the edge HUD reads as see-through. Vendor Mercury /
real glasses builds skip scale + camera underlay (optical AR). Prefer landscape.
UI is dark-only edge HUD (center clear until chat is expanded). Tune `FACTOR`
in `PhonePreviewScale.kt` if the UI is still too large (`↓`) or too small (`↑`).

```bash
cd src/apps/mobile/android
./gradlew :app:installDebug
```

## Manual verification

1. Desktop: show Remote Connect pairing QR (normal remote control, not account-auth).
2. Phone/glasses: grant camera → scan → auto-pair (no username UI) → AR split shell.
3. Toggle the sidebar chevron to collapse/expand the session rail.
4. Chat should track Desktop without re-selecting the session.
5. Device Wi‑Fi must reach `relay=` in the QR (`localhost` fails).

Logs: `adb logcat -s BitFunGlassesScan BitFunGlassesWeb *:E`
