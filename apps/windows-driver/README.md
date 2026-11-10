# Bridge Windows Indirect Display Driver (development foundation)

This directory contains the first **build-only** IddCx/UMDF driver foundation for Bridge Second Screen.

## What this milestone does

- Vendors Microsoft's official IddSampleDriver source as a pinned reference baseline.
- Gives the package a Bridge-specific root hardware ID and a unique UMDF DeviceGroupId.
- Builds the x64 UMDF driver package in GitHub Actions with a matched Windows SDK/WDK toolchain.
- Publishes the unsigned development package as a CI artifact for inspection.

## What it does not do yet

- It does not create or install a software device instance.
- It does not yet implement Bridge's phone-specific EDID/mode list or connect/disconnect control.
- It does not stream desktop frames to Android. The Microsoft sample's swap-chain loop is still a no-op.
- It is not signed for production and must not be installed on a daily-use Windows machine.

The source is intentionally kept close to Microsoft's sample so the first change is a reproducible build baseline. The next driver change will replace the sample monitor descriptors with a single Bridge monitor and implement an explicit, controlled software-device lifecycle before any user is asked to install it.

## Upstream source and license

- Repository: https://github.com/microsoft/Windows-driver-samples
- Sample: `video/IndirectDisplay/IddSampleDriver`
- Pinned upstream commit: `d49bb7d72f3651952b2b6e4ad28d14fe2247eb69`
- License: Microsoft Public License (MS-PL), reproduced in [THIRD_PARTY_LICENSE.txt](THIRD_PARTY_LICENSE.txt)

See Microsoft's [Indirect Display Driver sample notes](https://github.com/microsoft/Windows-driver-samples/tree/d49bb7d72f3651952b2b6e4ad28d14fe2247eb69/video/IndirectDisplay) for the sample's limitations and production TODOs.
