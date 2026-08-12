# BitFun Android (RayNeo X3 Glasses Companion)

Android APK for RayNeo X3 glasses (`com.bitfun.glasses`). Loads the standalone
`src/glasses-web` SPA from APK assets, while keeping the scanned relay URL for
same-origin `/api` calls.

## Scope

- **Scan** the Desktop remote-control QR (analysis-only camera, no preview HUD;
  optical see-through)
- **Binocular host**: `MobileWebActivity` extends Mercury `BaseMirrorActivity`;
  left eye runs the SPA, right eye is a live pixel clone (`EyeSurfaceMirror`)
  with left/right WebViews (optical see-through — no phone camera underlay)
- Auto-pair as `BitFun Glasses` → **page app** (~1:1 per eye via BindingPair)
- Sessions page and chat page are separate (not dual-rail HUD)
- Temple gestures → `window.__bitfunTemple` → SPA `[data-rayneo-focus]` ring
- Voice: RayNeo mic PCM → relay `transcribe_speech` → desktop Voice Input
  local ASR (whichever model is selected/installed in Desktop Settings) →
  auto-send in glasses-web. Desktop must have a local speech model installed;
  cloud Voice Input is not used for glasses remote transcription yet.
- Shared install id + session mirror across binocular WebViews (left eye pairs)

Explicitly **not** in this phase:

- Peer Device Mode (`HostInvoke`)
- Binocular QR scan page
- Desktop / relay / `src/mobile-web` changes
- Account-auth pairing on glasses
- Native Kotlin chat UI

## Flow

```text
QrScanActivity (BaseMirrorActivity)  →  MobileWebActivity (BaseMirrorActivity)
     left+right identical scan HUD        left + right WebView load pairing URL
     analysis-only camera                 assets from APK; /api on relay
                                          left eye pairs → native session mirror
                                          right eye restores mirror (no second /pair)
                                          temple → JS focus / click
```

## Mercury AAR

Place the vendor AAR under `app/libs/` (gitignored), e.g.
`MercuryAndroidSDK-v0.2.6-*.aar`. The build picks the first `*.aar`.

Without an AAR, the `mercury-stub` module supplies `BaseMirrorActivity` /
`TempleAction` stubs so CI and phone debug still compile.

## Build

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

## Manual verification (real glasses)

1. Desktop: show Remote Connect pairing QR (not account-auth).
2. Glasses: grant camera → scan → auto-pair → binocular edge HUD.
3. Temple slide moves the green focus ring; click activates the control.
4. Mic fills the composer; user still taps send.
5. Clear-view hides rails; restore brings them back.
6. Device Wi‑Fi must reach `relay=` in the QR (`localhost` fails).

Logs: `adb logcat -s BitFunGlassesScan BitFunGlassesWeb BitFunVoice BitFunGlassesHost *:E`
