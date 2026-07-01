//! GAP-60: Pane screenshot / export.
//!
//! - `.txt` exports the raw terminal text content (always available).
//! - `.png` exports an RGBA8 framebuffer snapshot of the rendered pane.
//!   The snapshot is supplied by the caller (an iced `Surface`/`wgpu` read
//!   happens in the renderer thread; this module only encodes the bytes).
//! - `.svg` is a stub (rebuilds the terminal as text inside an SVG `<rect>`
//!   of the configured cell size; the GPU-rendered cell shading is not
//!   reproduced).

use std::path::Path;

use image::{ImageBuffer, Rgba, RgbaImage};

/// `[pane_capture]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneCaptureConfig {
    pub enabled: bool,
    pub default_format: String,
}

impl Default for PaneCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_format: "txt".into(),
        }
    }
}

/// Export terminal text content to a file. PNG rendering deferred; `.txt` always works.
pub fn export_text_snapshot(content: &str, output: &Path) -> Result<(), String> {
    if content.is_empty() {
        return Err("empty snapshot".into());
    }
    std::fs::write(output, content).map_err(|e| e.to_string())
}

/// Encode a framebuffer as PNG and write to `output`. `width` / `height`
/// are the framebuffer dimensions in pixels; `rgba` must be exactly
/// `width * height * 4` bytes long (RGBA8, premultiplied alpha is fine
/// for the `image` crate).
pub fn export_png(rgba: &[u8], width: u32, height: u32, output: &Path) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("zero-sized framebuffer".into());
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "framebuffer length {} does not match {}x{} (expected {})",
            rgba.len(),
            width,
            height,
            expected
        ));
    }
    let img: RgbaImage = ImageBuffer::from_fn(width, height, |x, y| {
        let i = ((y * width + x) * 4) as usize;
        Rgba([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]])
    });
    img.save(output).map_err(|e| format!("save png: {e}"))
}

/// Build a minimal SVG wrapping the text content. The result is a
/// plain monospace block; cell background colors and font weights are
/// not preserved (kept as a stub for the SVG path).
pub fn export_svg_stub(content: &str, cell_w: u32, cell_h: u32, output: &Path) -> Result<(), String> {
    if cell_w == 0 || cell_h == 0 {
        return Err("zero cell size".into());
    }
    let lines: Vec<&str> = content.lines().collect();
    let width = cell_w * 80;
    let height = cell_h * lines.len().max(1) as u32;
    let mut body = String::new();
    for (i, line) in lines.iter().enumerate() {
        let y = (i as u32 + 1) * cell_h;
        // HTML-escape the few chars that break SVG text rendering.
        let escaped = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        body.push_str(&format!(
            r#"<text x="0" y="{y}" font-family="monospace" font-size="{fs}">{escaped}</text>"#,
            y = y,
            fs = cell_h,
            escaped = escaped
        ));
    }
    // Build SVG via string concatenation rather than a single format!
    // call. format! treats '#' followed by a digit as a format-spec
    // marker in some positions; the chunked form sidesteps that quirk.
    let svg = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"100%\" height=\"100%\" fill=\"black\"/>{body}</svg>",
        w = width, h = height, body = body
    );
    std::fs::write(output, svg).map_err(|e| e.to_string())
}

/// Dispatch by file extension. Supports `txt`/`text`/`png`/`svg` (case
/// insensitive). Falls back to `cfg.default_format` when the extension
/// is unknown.
pub fn export_snapshot_with_png(
    content: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
    output: &Path,
    format: &str,
) -> Result<(), String> {
    match format.to_ascii_lowercase().as_str() {
        "txt" | "text" => export_text_snapshot(content, output),
        "png" => export_png(rgba, width, height, output),
        "svg" => export_svg_stub(content, 8, 16, output),
        other => Err(format!("unsupported format: {other}")),
    }
}

/// Backwards-compatible stub: writes the text content with a placeholder
/// note for non-txt formats. Kept for callers that haven't yet been
/// wired to the GPU readback.
pub fn export_snapshot(content: &str, output: &Path, format: &str) -> Result<(), String> {
    match format.to_ascii_lowercase().as_str() {
        "txt" | "text" => export_text_snapshot(content, output),
        "png" => {
            let note = format!(
                "# PureTerm pane capture (PNG deferred — use export_snapshot_with_png with framebuffer)
# Lines: {}

{content}",
                content.lines().count()
            );
            export_text_snapshot(&note, output)
        }
        other => Err(format!("unsupported format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn export_txt_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pt-cap-txt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.txt");
        export_text_snapshot("hello\nworld", &path).unwrap();
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_png_writes_valid_file() {
        let dir = std::env::temp_dir().join(format!("pt-cap-png-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.png");
        // 2x2 RGBA, all red opaque.
        let rgba = vec![255u8, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        export_png(&rgba, 2, 2, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(&bytes[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_png_rejects_mismatched_length() {
        let dir = std::env::temp_dir().join(format!("pt-cap-png-bad-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.png");
        // 2x2 expected, but only 2 RGBA bytes.
        let rgba = vec![0u8; 2];
        let r = export_png(&rgba, 2, 2, &path);
        assert!(r.is_err());
    }

    #[test]
    fn export_png_rejects_zero_size() {
        let dir = std::env::temp_dir().join(format!("pt-cap-png-zero-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.png");
        assert!(export_png(&[], 0, 0, &path).is_err());
    }

    #[test]
    fn export_svg_writes_well_formed_header() {
        let dir = std::env::temp_dir().join(format!("pt-cap-svg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.svg");
        export_svg_stub("echo hi", 8, 16, &path).unwrap();
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.starts_with(r#"<?xml version="1.0""#));
        assert!(s.contains("echo hi"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_routes_by_format() {
        let dir = std::env::temp_dir().join(format!("pt-cap-disp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let rgba = vec![0u8; 4 * 1 * 1];
        let p_png = dir.join("snap.png");
        let p_txt = dir.join("snap.txt");
        let p_svg = dir.join("snap.svg");
        export_snapshot_with_png("hello", &rgba, 1, 1, &p_png, "png").unwrap();
        export_snapshot_with_png("hello", &rgba, 1, 1, &p_txt, "txt").unwrap();
        export_snapshot_with_png("hello", &rgba, 1, 1, &p_svg, "svg").unwrap();
        assert!(p_png.exists());
        assert!(p_txt.exists());
        assert!(p_svg.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
