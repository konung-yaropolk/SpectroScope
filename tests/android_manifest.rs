//! The Android packaging metadata has to say enough for the APK to launch.
//!
//! `cargo-apk2` generates `AndroidManifest.xml` out of
//! `[package.metadata.android]` in Cargo.toml, and it models
//! `application.activities` as a `Vec<Activity>` that defaults to **empty**.
//! When it is empty the tool prints one warning to stderr and carries on, so
//! the APK it produces has no `<activity>` element at all: it installs, no
//! launcher icon appears, and there is nothing for the system to start.
//! Release v0.1.0 shipped exactly that.
//!
//! Nothing else in this repository reads that table -- no build, no test, no
//! lint -- so these assertions are what stands between a careless edit and
//! another dud release. The release workflow decodes the manifest out of the
//! *built* APK as well; this is the cheap half that runs on every push.

use std::path::{Path, PathBuf};

use toml::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> Value {
    let text = std::fs::read_to_string(repo().join("Cargo.toml")).expect("read Cargo.toml");
    // `from_str`, not `text.parse()`: `FromStr for Value` parses a single TOML
    // *value*, so it reads `[package]` as an array and then trips over the rest
    // of the file.
    toml::from_str(&text).expect("Cargo.toml is valid TOML")
}

/// `a.b.c` into a TOML tree.
fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |acc, key| acc.get(key))
}

#[test]
fn the_android_manifest_can_be_launched() {
    let m = manifest();
    let app = at(&m, "package.metadata.android.application")
        .expect("[package.metadata.android.application] is missing");

    let activities = app.get("activity").and_then(Value::as_array).expect(
        "no [[package.metadata.android.application.activity]] -- the APK would have \
                 no <activity> element, so it would install with no launcher icon and \
                 could not be started",
    );
    assert!(!activities.is_empty(), "the activity list is empty");

    // The one the launcher will show.
    let launcher = activities
        .iter()
        .find(|a| {
            a.get("intent_filter")
                .and_then(Value::as_array)
                .is_some_and(|filters| {
                    filters.iter().any(|f| {
                        has(f, "actions", "android.intent.action.MAIN")
                            && has(f, "categories", "android.intent.category.LAUNCHER")
                    })
                })
        })
        .expect(
            "no activity has an intent filter with both the MAIN action and the \
                 LAUNCHER category, which is what puts the icon in the launcher",
        );

    let name = launcher.get("name").and_then(Value::as_str).unwrap_or("");
    assert!(
        !name.is_empty(),
        "the launcher activity has no name; cargo-apk2 defaults it to the empty \
         string, which is not a valid android:name"
    );

    // `ANativeActivity_onCreate` has to be findable, and it lives in the cdylib
    // named after `[lib]`. cargo-apk2 fills a missing value in from that name,
    // but we state it, so the two must agree.
    let lib_name = at(&m, "lib.name")
        .and_then(Value::as_str)
        .expect("[lib] name");
    let declared = launcher
        .get("meta_data")
        .and_then(Value::as_array)
        .and_then(|md| {
            md.iter()
                .find(|e| e.get("name").and_then(Value::as_str) == Some("android.app.lib_name"))
        })
        .expect("the launcher activity declares no android.app.lib_name meta-data");
    assert_eq!(
        declared.get("value").and_then(Value::as_str),
        Some(lib_name),
        "android.app.lib_name must name the cdylib, which is [lib] name"
    );
}

fn has(filter: &Value, key: &str, want: &str) -> bool {
    filter
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|xs| xs.iter().any(|x| x.as_str() == Some(want)))
}

/// The launcher icon has to exist on disk, in the buckets aapt2 reads.
///
/// `aapt2 compile --dir` is handed the whole tree and a missing file is not an
/// error there -- it becomes an `@mipmap/ic_launcher` that resolves to nothing
/// at link time, which fails late and cryptically.
#[test]
fn the_launcher_icon_is_where_the_manifest_says() {
    let m = manifest();
    let icon = at(&m, "package.metadata.android.application.icon")
        .and_then(Value::as_str)
        .expect("no application icon");
    let res = at(&m, "package.metadata.android.resources")
        .and_then(Value::as_str)
        .expect("no resources directory");

    let Some(name) = icon.strip_prefix("@mipmap/") else {
        panic!("expected an @mipmap/ icon reference, got {icon:?}");
    };

    let root = repo().join(res);
    assert!(root.is_dir(), "{} is not a directory", root.display());

    // Every bucket a current device may ask for, at the size that bucket
    // means. Leaving one out is not fatal -- Android rescales a neighbour --
    // but it is always a mistake rather than a decision.
    //
    // 48dp for the legacy bitmap, 108dp for the adaptive foreground: the
    // second is fixed by the platform, and getting it wrong is the difference
    // between an icon that fills its mask and one that floats inside it.
    for (bucket, scale) in [
        ("mdpi", 1.0),
        ("hdpi", 1.5),
        ("xhdpi", 2.0),
        ("xxhdpi", 3.0),
        ("xxxhdpi", 4.0),
    ] {
        let dir = root.join(format!("mipmap-{bucket}"));
        png_is(&dir.join(format!("{name}.png")), (48.0 * scale) as u32);
        png_is(
            &dir.join(format!("{name}_foreground.png")),
            (108.0 * scale) as u32,
        );
    }

    // The adaptive icon itself. Without it, every device that can install the
    // APK -- min_sdk is 26, so all of them -- applies "legacy treatment" to
    // the bitmap above: shrink it and composite it onto a generated white
    // plate, which for a full-bleed tile looks like a badge in a blob and is
    // a fair description of "there is no icon".
    let adaptive = root.join("mipmap-anydpi-v26").join(format!("{name}.xml"));
    let xml = std::fs::read_to_string(&adaptive)
        .unwrap_or_else(|e| panic!("{}: {e}", adaptive.display()));
    for needle in [
        "<adaptive-icon",
        "@color/ic_launcher_background",
        &format!("@mipmap/{name}_foreground"),
    ] {
        assert!(
            xml.contains(needle),
            "{} has no {needle}",
            adaptive.display()
        );
    }

    let colours = root.join("values").join("ic_launcher_background.xml");
    let text =
        std::fs::read_to_string(&colours).unwrap_or_else(|e| panic!("{}: {e}", colours.display()));
    assert!(
        text.contains("name=\"ic_launcher_background\""),
        "{} does not define the colour the adaptive icon references",
        colours.display()
    );
}

/// Assert `path` is a real PNG of exactly `size` square.
///
/// Reading the header rather than the file length, because "larger than a few
/// hundred bytes" passes for a truncated file, a JPEG renamed, or an icon
/// dropped into the wrong bucket -- and each of those fails inside `aapt2`
/// during a release instead of here.
fn png_is(path: &Path, size: u32) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "{} is not a PNG",
        path.display()
    );
    // IHDR is always the first chunk: 8 signature + 4 length + 4 type, then
    // width and height as big-endian u32.
    let at = |o: usize| u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap());
    let (w, h) = (at(16), at(20));
    assert_eq!(
        (w, h),
        (size, size),
        "{} is {w}x{h}, expected {size}x{size}",
        path.display()
    );
}
