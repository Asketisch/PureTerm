//! 把 `tests/ref/*/grid.json` 在明文 JSON 与 gzip 之间互相转换。
//!
//! 这些参考网格是高度重复的结构化 JSON(压缩比约 0.4%),明文入库会白占几十 MB。
//! 因此仓库里只存 `grid.json.gz`,测试读取时解压。
//!
//! 用法(`rustc -O -o /tmp/grid_json tools/grid_json.rs -L <deps> --extern flate2=<rlib>` 之后):
//! - `grid_json pack [目录...]`   明文 → gzip,删除明文
//! - `grid_json unpack [目录...]` gzip → 明文,便于人工比对
//!
//! 需在 crate 目录下运行(默认扫描 `tests/ref`);不传目录时处理所有用例目录。

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

const PLAIN: &str = "grid.json";
const PACKED: &str = "grid.json.gz";

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| usage());
    let rest: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let dirs = if rest.is_empty() { default_dirs()? } else { rest };

    match mode.as_str() {
        "pack" => {
            for dir in &dirs {
                pack(dir)?;
            }
        },
        "unpack" => {
            for dir in &dirs {
                unpack(dir)?;
            }
        },
        _ => usage(),
    }

    Ok(())
}

fn usage() -> ! {
    eprintln!("用法: grid_json <pack|unpack> [用例目录...]");
    std::process::exit(2);
}

fn default_dirs() -> io::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir("tests/ref")? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn pack(dir: &Path) -> io::Result<()> {
    let plain = dir.join(PLAIN);
    if !plain.exists() {
        return Ok(());
    }

    let raw = fs::read(&plain)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&raw)?;
    let packed = encoder.finish()?;

    fs::write(dir.join(PACKED), &packed)?;
    fs::remove_file(&plain)?;

    println!("{}: {} → {}", dir.display(), raw.len(), packed.len());
    Ok(())
}

fn unpack(dir: &Path) -> io::Result<()> {
    let packed_path = dir.join(PACKED);
    if !packed_path.exists() {
        return Ok(());
    }

    let mut raw = Vec::new();
    GzDecoder::new(File::open(&packed_path)?).read_to_end(&mut raw)?;
    fs::write(dir.join(PLAIN), raw)?;

    println!("{}: 已解压 {}", dir.display(), PLAIN);
    Ok(())
}
