# MMO Mouse Control

[日本語](../../README.md) · **English** · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md) · [한국어](README.ko.md) · [Español](README.es.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Português](README.pt.md)

A Windows 10/11 (x64) utility for configuring multi-button mice. Set button assignments, DPI, lighting, and onboard profiles, and back up or restore settings.

## Installation

Download **MMOMouseControl-Setup-0.12.2-x64.exe** from [GitHub Releases](https://github.com/nekomimiyks/mmo-mouse-control/releases/latest) and run it. If you used the portable version, select its folder during setup. At the end, choose **Yes** to start now or **No** to close setup.

When updating, setup closes the running installed copy before replacing it. If the app is waiting for your response, finish the prompt and run setup again.

## What’s new in 0.12.2

Windows installer, portable settings migration, and a single backup file for app settings.

## Back up and restore settings

Open **App settings backup → Save backup** to export saved drafts, custom shortcuts, and display language to a `.mmo-settings` file. To restore, select the file with **Restore from backup**, confirm, then reopen the app. Use each mouse’s own backup controls for onboard profiles.

## Compatibility

- Tested on hardware: wired Redragon M908 Impact and Logitech G600r (Logicool G600r in Japan).
- Likely compatible based on published USB IDs: wired [Monster Pusat V8 MMO](https://github.com/dokutan/mouse_m908/blob/965d71f957023ebdfe9bc3ea8d2affbfb21c10ff/README.md#supported-mice) and [Logitech G600](https://github.com/libratbag/piper/blob/master/data/svgs/svg-lookup.ini). These models have not been tested with this app.

## App languages

Japanese, English, Simplified Chinese, Traditional Chinese, Korean, Spanish, French, German, and Portuguese.

Open **G600 / G600r → Settings → Language** to choose a language. **Automatic / Windows** uses the Windows display language and falls back to English when the language is unavailable. This setting applies to the G600 / G600r screen. The M908 / Pusat V8 screen is in Japanese.

## Screenshots

Screenshots show sample settings.

### App settings backup

![App settings backup](../../screenshots/localized/en/backup.png)

### G600r settings

![G600r settings](../../screenshots/localized/en/settings.png)

### G600r button assignments

![G600r button assignments](../../screenshots/localized/en/buttons.png)

## Build from source

On Windows, prepare the Rust GNU toolchain, the MinGW resource compiler, and Cargo dependencies. Run **./build.ps1** in PowerShell.

[License](../../LICENSE) · [Third-party notices](../../THIRD_PARTY.md) · [Privacy policy](../../PRIVACY.md)
