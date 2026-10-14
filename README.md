# MangoDisk — Android Edition

<p align="center">
  <img alt="Android supported" src="https://img.shields.io/badge/Android-supported-3ddc84?logo=android&logoColor=white">
  <img alt="Rust core" src="https://img.shields.io/badge/core-Rust-b7410e?logo=rust&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white">
</p>

**MangoDisk for Android** is a real, working disk cleaner for Android phones —
not a UI shell. It scans shared storage and cleans what you approve, with a
live progress view and a per-category report at the end.

## What it cleans

- **Temp files** — leftover temporary files across shared storage
- **Logs** — stale log files
- **Caches** — app and web caches on shared storage
- **Empty directories** — left-behind empty folders
- **Large files (200 MB+)** — find and review the biggest space hogs
- **App cache** — clear your own app's cache in one tap

Everything runs on-device under your explicit All-Files-Access permission;
nothing is uploaded anywhere. You review categories before deleting, and the
clean report shows exactly what was removed.

## Install

1. Grab the APK from [Releases](../../releases) (e.g. `MangoDisk-1.1.7-android-arm64.apk`).
2. Allow "Install unknown apps" for your browser/file manager.
3. Launch MangoDisk and grant **All files access** when prompted.

Built from the upstream Tauri 2 app with a Rust cleaning engine
(`mobile_cleaner.rs`) and a small Android bridge (`MangoDiskBridge`) for the
All-Files-Access flow.

## Credits

This fork is built on top of **[MangoDisk by harry0703](https://github.com/harry0703/MangoDisk)** —
all credit for the original MangoDisk concept, desktop app (macOS / Windows /
Linux), and design goes to the original author. This repository only adds the
Android packaging and the mobile cleaning engine.

## Notes

- The APK is debug-signed for personal install; it is not Play-Store signed.
- On Android 11+ other apps' `Android/data` folders are kernel-locked; the
  cleaner works on shared storage and its own cache (by platform design).
- The original desktop app lives at the upstream repository above.
