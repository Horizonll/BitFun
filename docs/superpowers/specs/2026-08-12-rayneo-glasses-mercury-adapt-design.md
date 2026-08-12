# RayNeo X3 real-glasses MercurySDK adaptation

Date: 2026-08-12  
Status: approved (approach A)  
Reference: `MercurySDK_Skill_Reference_CN.md`, vendor AAR `MercuryAndroidSDK-v0.2.6-*.aar`

## Goal

Move BitFun glasses companion from phone-simulated AR to real RayNeo X3:

- Optical see-through (no camera underlay)
- Binocular mirror UI via `BaseMirrorActivity`
- Temple gestures for navigation / activation
- RayNeo `VOICE_RECOGNITION` mic path for ASR

Keep `glasses-web` SPA. Do not change Desktop / relay / `mobile-web`.

## Architecture

```text
BitFunGlassesApp → MercurySDK.init()

QrScanActivity (single-screen camera scan, no phone scale)
        ↓
MobileWebActivity : BaseMirrorActivity<ActivityMobileWebBinding>
  left WebView ─┐
                ├─ same relay URL + assets intercept
  right WebView ┘
  TempleAction → JS __bitfunTemple
  Voice → RayNeo PCM + HiAI writePcm → JS voice events
```

## Decisions

| Topic | Choice |
|---|---|
| UI host | `BaseMirrorActivity` + dual WebView (A) |
| QR scan | Single-screen this phase |
| Focus | Temple → JS; SPA owns `[data-rayneo-focus]` ring (dynamic DOM) |
| Voice | Always RayNeo mic params + HiAI PCM on glasses |
| Phone preview | Remove camera underlay + disable `PhonePreviewScale` for vendor builds |
| AAR | Local `app/libs/*.aar` (gitignored); build picks first Mercury AAR |

## Out of scope

- Native rewrite of chat UI
- Peer Device Mode / HostInvoke
- Binocular QR scan page
- Desktop / relay changes
