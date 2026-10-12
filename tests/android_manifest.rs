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
/// `aapt2 compile --dir` is given the whole tree, and a missing file is not an
/// error there -- it is an `@mipmap/ic_launcher` that resolves to nothing at
/// link time, which fails the build late and cryptically.
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

    // Every bucket a current Android device may ask for. Leaving one out is
    // not fatal -- Android rescales a neighbour -- but it is always a mistake
    // rather than a decision, so all five are required.
    for bucket in ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"] {
        let p: &Path = &root
            .join(format!("mipmap-{bucket}"))
            .join(format!("{name}.png"));
        assert!(p.is_file(), "missing {}", p.display());
        let bytes = std::fs::metadata(p).expect("stat").len();
        assert!(
            bytes > 256,
            "{} is suspiciously small ({bytes} bytes)",
            p.display()
        );
    }
}
