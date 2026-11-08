// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore listviewitem stylemetrics cdtor QTBUG

#[cfg(feature = "enable")]
fn main() {
    println!("cargo:rustc-check-cfg=cfg(no_qt)");
    println!("cargo:rustc-check-cfg=cfg(slint_nightly_test)");

    println!("cargo:rerun-if-env-changed=SLINT_NO_QT");
    assert!(
        std::env::var_os("SLINT_NO_QT").is_none(),
        "The native Qt desktop profile cannot disable Qt with SLINT_NO_QT"
    );
    assert!(
        !std::env::var("TARGET").is_ok_and(|t| t.starts_with("wasm")),
        "The native Qt desktop profile requires a native target"
    );
    assert!(
        std::env::var("DEP_QT_FOUND").is_ok_and(|found| found == "1"),
        "Qt development files were not found. Install Qt 6.12 or later in the Qt 6 series and set QMAKE to its qmake executable. {}",
        std::env::var("DEP_QT_ERROR_MESSAGE").unwrap_or_default()
    );
    let qt_version = std::env::var("DEP_QT_VERSION").expect("Qt version was not reported");
    let mut version_parts = qt_version.split('.');
    let supported = version_parts.next() == Some("6")
        && version_parts.next().is_some_and(|minor| minor.parse::<u32>().is_ok_and(|minor| minor >= 12));
    assert!(supported, "Unsupported Qt {qt_version}: install Qt 6.12 or later in the Qt 6 series");

    println!("cargo:rustc-env=EML_QT_BUILD_VERSION={qt_version}");
    let mut config = cpp_build::Config::new();
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux")
        && std::env::var_os("CARGO_FEATURE_RAW_WINDOW_HANDLE_06").is_some()
    {
        config.define("EML_QT_RAW_WINDOW_HANDLE", None);
        let includes = std::env::var("DEP_QT_INCLUDE_PATH").unwrap();
        config.include(format!("{includes}/QtGui/{qt_version}/QtGui"));
    }
    for f in std::env::var("DEP_QT_COMPILE_FLAGS").unwrap().split_terminator(';') {
        config.flag(f);
    }
    config.flag_if_supported("-std=c++17");
    config.flag_if_supported("/std:c++17");
    // Workaround QTBUG-123153
    config.flag_if_supported("-Wno-template-id-cdtor");
    // On some systems, the header GL/gl.h (included by some Qt headers) is not
    // in the compilers default include path, which makes the build fail due to
    // this header not being found. As we don't need OpenGL, we explicitly
    // disable it with this define. See issue #10989.
    config.define("QT_NO_OPENGL", None);
    config.include(std::env::var("DEP_QT_INCLUDE_PATH").unwrap()).build("lib.rs");

    println!("cargo:rerun-if-changed=lib.rs");
    println!("cargo:rerun-if-changed=native_dialogs.rs");
    println!("cargo:rerun-if-changed=qt_accessible.rs");
    println!("cargo:rerun-if-changed=qt_accessible/text.rs");
    println!("cargo:rerun-if-changed=qt_accessible/popups.rs");
    println!("cargo:rerun-if-changed=qt_widgets.rs");
    println!("cargo:rerun-if-changed=qt_widgets/button.rs");
    println!("cargo:rerun-if-changed=qt_widgets/checkbox.rs");
    println!("cargo:rerun-if-changed=qt_widgets/combobox.rs");
    println!("cargo:rerun-if-changed=qt_widgets/groupbox.rs");
    println!("cargo:rerun-if-changed=qt_widgets/lineedit.rs");
    println!("cargo:rerun-if-changed=qt_widgets/listviewitem.rs");
    println!("cargo:rerun-if-changed=qt_widgets/scrollview.rs");
    println!("cargo:rerun-if-changed=qt_widgets/slider.rs");
    println!("cargo:rerun-if-changed=qt_widgets/progress_indicator.rs");
    println!("cargo:rerun-if-changed=qt_widgets/spinbox.rs");
    println!("cargo:rerun-if-changed=qt_widgets/stylemetrics.rs");
    println!("cargo:rerun-if-changed=qt_widgets/palette.rs");
    println!("cargo:rerun-if-changed=qt_widgets/tabwidget.rs");
    println!("cargo:rerun-if-changed=qt_widgets/tableheadersection.rs");
    println!("cargo:rerun-if-changed=qt_window.rs");
    println!("cargo:SUPPORTS_NATIVE_STYLE=1");
}

#[cfg(not(feature = "enable"))]
fn main() {
    println!("cargo:rustc-cfg=no_qt");
    println!("cargo:rustc-check-cfg=cfg(no_qt)");
    println!("cargo:rustc-check-cfg=cfg(slint_nightly_test)");
    return;
}
