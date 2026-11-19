# MMO Mouse Control

[日本語](../../README.md) · **English** · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md) · [한국어](README.ko.md) · [Español](README.es.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Português](README.pt.md)

A Windows 10/11 (x64) utility for configuring multi-button mice. Set button assignments, DPI, lighting, and onboard profiles, and back up or restore settings.

## Download

Connect a supported mouse by USB, download **MMOMouseControl.exe** from [GitHub Releases](https://github.com/nekomimiyks/mmo-mouse-control/releases/latest), and run it.

## Compatibility

- Tested on hardware: wired Redragon M908 Impact and Logitech G600r (Logicool G600r in Japan).
- Likely compatible based on published USB IDs: wired [Monster Pusat V8 MMO](https://github.com/dokutan/mouse_m908/blob/965d71f957023ebdfe9bc3ea8d2affbfb21c10ff/README.md#supported-mice) and [Logitech G600](https://github.com/libratbag/piper/blob/master/data/svgs/svg-lookup.ini). These models have not been tested with this app.

## App languages

Japanese, English, Simplified Chinese, Traditional Chinese, Korean, Spanish, French, German, and Portuguese.

Open **G600 / G600r → Settings → Language** to choose a language. **Automatic / Windows** uses the Windows display language and falls back to English when the language is unavailable. This setting applies to the G600 / G600r screen. The M908 / Pusat V8 screen is in Japanese.

## Screenshots

| G600r device settings | G600r button assignments |
| --- | --- |
| ![G600r device settings](../../screenshots/device-settings.jpg) | ![G600r button assignments](../../screenshots/button-mapping.jpg) |

## Build from source

On Windows, prepare the Rust GNU toolchain, the MinGW resource compiler, and Cargo dependencies. Run **./build.ps1** in PowerShell.

[License](../../LICENSE) · [Third-party notices](../../THIRD_PARTY.md) · [Privacy policy](../../PRIVACY.md)
