---
name: run-android
description: Build, install, launch, drive and screenshot the Resonate Android app on an emulator from the agent shell. Use whenever the Compose UI needs to be seen or exercised for real, after any change to android/app, or to give the emulator a demo music library.
---

# Running the Resonate Android app

The SDK is at `$HOME/Android/Sdk`; `adb` and `emulator` are not on the agent shell's PATH.

```bash
export ANDROID_HOME=$HOME/Android/Sdk
export PATH=$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH
adb devices
emulator -list-avds
```

If no device is listed, start the first AVD in the background (`emulator -avd <name> -no-snapshot-save &`)
and wait for `adb wait-for-device` and `adb shell getprop sys.boot_completed` to answer `1`.

## Build, install, launch

From the repository root (Gradle builds the Rust library and the Kotlin bindings first):

```bash
android/gradlew -p android :app:installDebug -q
adb shell am force-stop org.resonate.android
adb shell am start -n org.resonate.android/.MainActivity
```

## See it

```bash
adb exec-out screencap -p > "$SCRATCH/shot.png"
```

Read the PNG. Coordinates for `adb shell input tap X Y` are in the device's pixels (1080×2400 on a
medium phone). Every key element carries a `testTag` exported as its resource id
(`testTagsAsResourceId`), so bounds come from the hierarchy:

```bash
adb shell uiautomator dump /sdcard/ui.xml >/dev/null && adb shell cat /sdcard/ui.xml
```

Check both themes and large text, then put them back:

```bash
adb shell cmd uimode night yes        # no to return
adb shell settings put system font_scale 2.0   # 1.0 to return
```

## A library to look at

An empty emulator shows the empty states. To give it songs, generate tagged files with covers on
the host (`ffmpeg` sine tones, `metaflac --import-picture-from`), push them under
`/sdcard/Download/<folder>/`, index them and grant access:

```bash
adb push "<album folder>" "/sdcard/Download/Resonate Demo/"
adb shell content call --method scan_volume --uri content://media --arg external_primary
adb shell pm grant org.resonate.android android.permission.READ_MEDIA_AUDIO
```

Then choose **Use Downloads** in the app (Library's menu or Settings → Music folders).

## Several agents, one emulator

Installs replace each other's build. Hold a lock for each whole install-drive-screenshot sequence:
`flock "$SCRATCH/emulator.lock" bash -c '…'`.
