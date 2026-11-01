//! Conservative CPU/RAM/CUDA planning for GGUF inference.
//! CUDA free memory is queried at launch; estimates are not hard allocation limits.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

pub const GPU_BALANCED: u32 = 1023;
pub const GPU_AUTO: u32 = 1024;
const GIB: u64 = 1024 * 1024 * 1024;
const MAX_META_BYTES: u64 = 128 * 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn read_u32(file: &mut File) -> io::Result<u32> {
    let mut bytes = [0; 4];
    file.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_u64(file: &mut File) -> io::Result<u64> {
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}
fn skip(file: &mut File, length: u64) -> io::Result<()> {
    let next = file.stream_position()?.checked_add(length)
        .ok_or_else(|| invalid("GGUF metadata overflow"))?;
    if next > MAX_META_BYTES || next > file.metadata()?.len() {
        return Err(invalid("GGUF metadata outside safe scan limit"));
    }
    file.seek(SeekFrom::Start(next))?;
    Ok(())
}
fn skip_value(file: &mut File, ty: u32) -> io::Result<Option<u64>> {
    match ty {
        0 | 1 | 7 => { skip(file, 1)?; Ok(None) }
        2 | 3 => { skip(file, 2)?; Ok(None) }
        4 => Ok(Some(read_u32(file)? as u64)),
        5 | 6 => { skip(file, 4)?; Ok(None) }
        8 => { let n = read_u64(file)?; skip(file, n)?; Ok(None) }
        9 => {
            let element = read_u32(file)?;
            let count = read_u64(file)?;
            if count > 2_000_000 { return Err(invalid("GGUF array too large")); }
            let width: Option<u64> = match element {
                0 | 1 | 7 => Some(1),
                2 | 3 => Some(2),
                4 | 5 | 6 => Some(4),
                10 | 11 | 12 => Some(8),
                _ => None,
            };
            if let Some(width) = width {
                skip(file, count.checked_mul(width).ok_or_else(|| invalid("GGUF array overflow"))?)?;
            } else if element == 8 {
                for _ in 0..count {
                    let len = read_u64(file)?;
                    skip(file, len)?;
                }
            } else {
                return Err(invalid("Unsupported GGUF array element type"));
            }
            Ok(None)
        }
        10 => Ok(Some(read_u64(file)?)),
        11 | 12 => { skip(file, 8)?; Ok(None) }
        _ => Err(invalid("Unknown GGUF metadata type")),
    }
}

/// Read only the GGUF metadata; no tensor weights are allocated.
pub fn gguf_block_count(path: &Path) -> io::Result<u32> {
    let mut file = File::open(path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"GGUF" || ![2, 3].contains(&read_u32(&mut file)?) {
        return Err(invalid("Unsupported GGUF version"));
    }
    let _tensors = read_u64(&mut file)?;
    let items = read_u64(&mut file)?;
    if items > 50_000 { return Err(invalid("Too many GGUF metadata entries")); }
    for _ in 0..items {
        let len = read_u64(&mut file)?;
        if len > 256 { return Err(invalid("GGUF metadata key too large")); }
        let mut key = vec![0u8; len as usize];
        file.read_exact(&mut key)?;
        let name = std::str::from_utf8(&key).map_err(|_| invalid("Invalid GGUF key"))?;
        let ty = read_u32(&mut file)?;
        let count = skip_value(&mut file, ty)?;
        if (name.ends_with(".block_count") || name.ends_with(".layer_count"))
            && let Some(count) = count
            && let Ok(count) = u32::try_from(count)
            && (1..=1022).contains(&count)
        {
            return Ok(count);
        }
    }
    Err(invalid("GGUF block count not found. Select a manual CUDA layer count."))
}

/// Reserve at least 1.5 GiB, plus a modest context-dependent buffer.
/// This is a conservative approximation, not a guarantee against CUDA OOM.
pub fn gpu_weight_budget(free_vram: u64, context: u32) -> u64 {
    let reserve = (free_vram / 5)
        .max(1536 * 1024 * 1024)
        .saturating_add((context as u64 / 4096) * 256 * 1024 * 1024);
    free_vram.saturating_sub(reserve)
}

/// Partial offload based on available VRAM and GGUF weight size.
/// Auto prioritizes GPU; Balanced deliberately retains CPU work.
pub fn recommended_layers(
    model_bytes: u64,
    blocks: u32,
    free_vram: u64,
    context: u32,
    balanced: bool,
) -> u32 {
    if model_bytes == 0 || blocks == 0 { return 0; }
    let budget = gpu_weight_budget(free_vram, context);
    let per_layer = (model_bytes / u64::from(blocks)).max(1);
    let safe_per_layer = per_layer.saturating_mul(135) / 100;
    let fitted = (budget / safe_per_layer.max(1)).min(u64::from(blocks)) as u32;
    if balanced {
        fitted.min((blocks * 2 / 3).max(1).min(blocks.saturating_sub(1)))
    } else {
        fitted
    }
}

pub fn memory_preflight(model_bytes: u64, free_ram: Option<u64>, gpu_budget: u64) -> io::Result<()> {
    if let Some(free_ram) = free_ram {
        let usable_ram = free_ram.saturating_sub(2 * GIB);
        if model_bytes > usable_ram.saturating_add(gpu_budget) {
            return Err(io::Error::other(format!(
                "GGUF {:.1} GiB превышает доступный бюджет RAM + VRAM (~{:.1} GiB). \
                 Выберите меньшую модель или HTTP Gateway; подкачка может вызвать сильное замедление.",
                model_bytes as f64 / GIB as f64,
                usable_ram.saturating_add(gpu_budget) as f64 / GIB as f64,
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qwen_sized_model_fits_cuda_or_balanced_with_headroom() {
        let size = 6 * GIB;
        assert!(recommended_layers(size, 32, 7 * GIB, 2048, false) > 0);
        assert!(recommended_layers(size, 32, 7 * GIB, 2048, true) <= 21);
        assert!(recommended_layers(size, 32, 7 * GIB, 8192, false)
            <= recommended_layers(size, 32, 7 * GIB, 2048, false));
        assert_eq!(recommended_layers(size, 32, 0, 4096, false), 0);
    }
    #[test]
    fn very_large_models_fail_preflight() {
        assert!(memory_preflight(130 * GIB, Some(30 * GIB), 6 * GIB).is_err());
        assert!(memory_preflight(6 * GIB, Some(24 * GIB), 4 * GIB).is_ok());
    }
    #[test]
    fn read_blocks_from_minimal_gguf_metadata() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qwen.gguf");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"GGUF").unwrap();
        file.write_all(&3u32.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(&1u64.to_le_bytes()).unwrap();
        let key = b"qwen35.block_count";
        file.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
        file.write_all(key).unwrap();
        file.write_all(&4u32.to_le_bytes()).unwrap();
        file.write_all(&32u32.to_le_bytes()).unwrap();
        drop(file);
        assert_eq!(gguf_block_count(&path).unwrap(), 32);
    }
}
