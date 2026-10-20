//! Native CoreText comparisons for the shipping software text layout and rasterizer.
#![cfg(all(target_os = "macos", feature = "macos-gui"))]
use slint::{
    ComponentHandle, Rgb8Pixel,
    platform::{
        Platform, PlatformError, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

#[allow(unsafe_code, reason = "quality:slint-generated-font-fixture")]
mod view {
    slint::slint! {
        export component FontObservation inherits Window {
            background: white;
            in property <string> family;
            in property <float> size;
            in property <int> weight;
            in property <string> sample;
            default-font-family: family;
            default-font-size: size * 1px;
            out property <length> x-height: glyph.font-metrics.x-height;
            out property <length> cap-height: glyph.font-metrics.cap-height;
            out property <length> ascent: glyph.font-metrics.ascent;
            out property <length> descent: glyph.font-metrics.descent;
            out property <length> advance: glyph.preferred-width;
            glyph := Text { x: 8px; y: 8px; width: parent.width - 16px; height: 80px; text: root.sample; color: black; font-weight: root.weight; }

        }
    }
}
struct Offscreen(Rc<MinimalSoftwareWindow>);
impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}
fn checked(command: &mut Command) -> Vec<u8> {
    let result = command.output().expect("Run native font observation tool");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}
fn near(actual: f32, expected: f64, tolerance: f64, context: &str) {
    assert!(
        (f64::from(actual) - expected).abs() <= tolerance,
        "{context}: Slint={actual}, CoreText={expected}"
    );
}
fn ink(buffer: &[Rgb8Pixel], stride: usize, rows: usize) -> (usize, usize) {
    let points: Vec<_> = buffer
        .iter()
        .take(stride * rows)
        .enumerate()
        .filter(|(_, p)| u32::from(p.r) + u32::from(p.g) + u32::from(p.b) < 765)
        .map(|(i, _)| (i % stride, i / stride))
        .collect();
    assert!(
        !points.is_empty(),
        "Software renderer produced no glyph pixels"
    );
    let width =
        points.iter().map(|p| p.0).max().unwrap() - points.iter().map(|p| p.0).min().unwrap() + 1;
    let height =
        points.iter().map(|p| p.1).max().unwrap() - points.iter().map(|p| p.1).min().unwrap() + 1;
    (width, height)
}
fn ppm(path: &Path, pixels: &[Rgb8Pixel], stride: usize, width: usize, height: usize) {
    let mut data = format!("P6\n{width} {height}\n255\n").into_bytes();
    for row in pixels.chunks_exact(stride).take(height) {
        for p in &row[..width] {
            data.extend_from_slice(&[p.r, p.g, p.b]);
        }
    }
    fs::write(path, data).unwrap();
}
fn compare_metrics(probe: &view::FontObservation, record: &serde_json::Value, context: &str) {
    near(
        probe.get_x_height(),
        record["x_height"].as_f64().unwrap(),
        0.02,
        &format!("x-height {context}"),
    );
    near(
        probe.get_cap_height(),
        record["cap_height"].as_f64().unwrap(),
        0.02,
        &format!("cap-height {context}"),
    );
    near(
        probe.get_ascent(),
        record["ascent"].as_f64().unwrap(),
        0.02,
        &format!("ascent {context}"),
    );
    near(
        probe.get_descent().abs(),
        record["descent"].as_f64().unwrap(),
        0.02,
        &format!("descent {context}"),
    );
    near(
        probe.get_advance(),
        record["advance"].as_f64().unwrap(),
        1.1,
        &format!("shaped advance {context}"),
    );
}
fn coretext_observations(root: &Path, output: &Path) -> serde_json::Value {
    let tool = output.join("NativeFontObservation");
    checked(
        Command::new("/usr/bin/xcrun")
            .args(["swiftc", "-parse-as-library"])
            .arg(root.join("tests/macos/NativeFontObservation.swift"))
            .arg("-o")
            .arg(&tool),
    );
    let bytes = checked(Command::new(&tool).arg(output));
    fs::write(output.join("coretext.json"), &bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
#[test]
fn system_optical_size_metrics_shaping_and_ink_bounds_match_coretext_at_both_densities() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let parent = root.join("target/native-font-checks");
    fs::create_dir_all(&parent).unwrap();
    let output = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    eprintln!("Native font QA retained at {}", output.display());
    let records = coretext_observations(&root, &output);
    let adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen(adapter.clone()))).unwrap();
    let probe = view::FontObservation::new().unwrap();
    let mut fonts = fontique::Collection::new(fontique::CollectionOptions::default());
    let id = fonts
        .generic_families(fontique::GenericFamily::SystemUi)
        .next()
        .unwrap();
    let family = fonts.family_name(id).unwrap().to_owned();
    probe.show().unwrap();
    let mut observations = Vec::new();
    let mut raster_failures = Vec::new();
    for record in records.as_array().unwrap() {
        let size = record["size"].as_f64().unwrap();
        let weight = record["weight"].as_i64().unwrap();
        let sample = record["sample"].as_str().unwrap();
        probe.set_family(
            if record["family"] == "system" {
                family.as_str()
            } else {
                "Helvetica"
            }
            .into(),
        );
        probe.set_size(size as f32);
        probe.set_weight(weight as i32);
        probe.set_sample(sample.into());
        let mut widths = Vec::new();
        for scale in [1usize, 2] {
            probe
                .window()
                .dispatch_event(WindowEvent::ScaleFactorChanged {
                    scale_factor: scale as f32,
                });
            probe
                .window()
                .set_size(slint::LogicalSize::new(4096.0, 220.0));
            slint::platform::update_timers_and_animations();
            let context = format!(
                "{} weight{weight} {size}pt @{scale}x {:?}",
                record["family"], sample
            );
            compare_metrics(&probe, record, &context);
            widths.push(probe.get_advance());
            let physical = probe.window().size();
            let stride = physical.width as usize;
            let mut buffer = vec![Rgb8Pixel::new(255, 255, 255); stride * physical.height as usize];
            adapter.request_redraw();
            assert!(adapter.draw_if_needed(|renderer| {
                renderer.render(&mut buffer, stride);
            }));
            // Compare full ink support. A midpoint intensity threshold would
            // compare gamma/smoothing policies, not the selected glyph extent.
            let bounds = ink(&buffer, stride, 90 * scale);
            let native = &record["raster"][scale - 1]["ink"];
            for (actual, expected) in [
                (bounds.0, native[0].as_u64().unwrap() as usize),
                (bounds.1, native[1].as_u64().unwrap() as usize),
            ] {
                if actual.abs_diff(expected) > 1 {
                    raster_failures.push(format!(
                        "raster {context}: software {bounds:?}, CoreText {native}"
                    ));
                }
            }
            if record["family"] == "system" && weight == 400 && sample == "x" {
                ppm(
                    &output.join(format!("software-{size}-{scale}x.ppm")),
                    &buffer,
                    stride,
                    ((record["advance"].as_f64().unwrap() + 16.0).ceil() as usize) * scale,
                    90 * scale,
                );
            }
            observations.push(serde_json::json!({"case":context,"x_height":probe.get_x_height(),"advance":probe.get_advance(),"ink":bounds}));
        }
        assert!(
            (widths[0] - widths[1]).abs() <= 0.6,
            "Logical width changed with density: {widths:?}"
        );
    }
    assert_eq!(observations.len(), 54, "Font scenario discovery changed");
    fs::write(
        output.join("software.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    probe.hide().unwrap();
    assert!(raster_failures.is_empty(), "{}", raster_failures.join("\n"));
}
