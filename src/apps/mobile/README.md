# BitFun Native Mobile Apps

This directory contains the native mobile product surfaces for BitFun:

- `android/`: Android application code and resources.
- `ios/`: iOS application code and resources.
- `harmonyos/`: HarmonyOS application code and resources.

Each platform directory owns its native UI, lifecycle, permissions, packaging,
and platform adapters. Product logic and stable contracts should remain in the
platform-agnostic Rust layers and be exposed to these apps through explicit
interfaces.

`android/` hosts the RayNeo X3 Pro remote-control companion Gradle project
(`com.bitfun.glasses`). See `android/README.md` for build and MVP scope.
`ios/` remains a placeholder until its stack is selected.
