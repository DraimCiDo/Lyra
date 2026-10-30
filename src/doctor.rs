use serde::Serialize;
use std::io::{self, Write};
use sysinfo::{CpuRefreshKind, Disks, Gpus, MemoryRefreshKind, RefreshKind, System};

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub lyra_version: &'static str,
    pub os: Option<String>,
    pub architecture: &'static str,
    pub cpu: CpuReport,
    pub memory: MemoryReport,
    /// None means the query failed. An empty list does not prove absence of hardware.
    pub gpus: Option<Vec<GpuReport>>,
    pub disks: Option<Vec<DiskReport>>,
    pub inference_status: &'static str,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CpuReport {
    pub brand: Option<String>,
    pub logical_cpus: Option<usize>,
    pub available_parallelism: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct MemoryReport {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct GpuReport {
    pub model: Option<String>,
    pub vendor: Option<String>,
    /// Dedicated VRAM only; shared/unified memory is not added to this number.
    pub dedicated_vram_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct DiskReport {
    pub mount_point: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// One local snapshot. No process lists, network inventory, shell or background loop.
pub fn collect() -> io::Result<DoctorReport> {
    let refresh = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing())
        .with_memory(MemoryRefreshKind::nothing().with_ram());
    let system = System::new_with_specifics(refresh)
        .map_err(|error| io::Error::other(format!("не удалось прочитать CPU/RAM: {error}")))?;
    let mut warnings = Vec::new();
    let os = System::long_os_version().ok();
    if os.is_none() {
        warnings.push("Версия ОС недоступна.".into());
    }
    let brand = system.cpus().first().and_then(|cpu| {
        let brand = cpu.brand().trim();
        (!brand.is_empty()).then(|| brand.to_owned())
    });
    let logical_cpus = match system.cpus().len() {
        0 => None,
        count => Some(count),
    };
    if brand.is_none() || logical_cpus.is_none() {
        warnings.push("Часть сведений о CPU недоступна.".into());
    }
    let total_bytes = match system.total_memory() {
        0 => None,
        bytes => Some(bytes),
    };
    if total_bytes.is_none() {
        warnings.push("Объём RAM недоступен.".into());
    }
    let memory = MemoryReport {
        total_bytes,
        available_bytes: total_bytes.map(|_| system.available_memory()),
    };
    let gpus = match Gpus::new_with_refreshed_list() {
        Ok(gpus) => {
            let devices: Vec<_> = gpus
                .iter()
                .map(|gpu| GpuReport {
                    model: gpu.model().map(str::to_owned),
                    vendor: gpu.vendor().map(str::to_owned),
                    dedicated_vram_bytes: gpu.total_memory(),
                })
                .collect();
            if devices.is_empty() {
                warnings.push(
                    "GPU не обнаружен диагностикой; это не доказывает его отсутствие.".into(),
                );
            } else if devices.iter().any(|gpu| gpu.dedicated_vram_bytes.is_none()) {
                warnings.push(
                    "Для части GPU объём выделенной VRAM недоступен; общая память не равна VRAM."
                        .into(),
                );
            }
            Some(devices)
        }
        Err(_) => {
            warnings.push("Сведения о GPU/VRAM недоступны в этой среде.".into());
            None
        }
    };
    let disks = match Disks::new_with_refreshed_list() {
        Ok(disks) => {
            let devices: Vec<_> = disks
                .iter()
                .map(|disk| DiskReport {
                    mount_point: disk.mount_point().to_string_lossy().into_owned(),
                    kind: disk.kind().to_string(),
                    total_bytes: disk.total_space(),
                    available_bytes: disk.available_space(),
                })
                .collect();
            if devices.is_empty() {
                warnings.push("Список дисков недоступен или пуст.".into());
            }
            Some(devices)
        }
        Err(_) => {
            warnings.push("Сведения о дисках недоступны в этой среде.".into());
            None
        }
    };
    Ok(DoctorReport {
        schema_version: 1,
        lyra_version: env!("CARGO_PKG_VERSION"),
        os,
        architecture: std::env::consts::ARCH,
        cpu: CpuReport {
            brand,
            logical_cpus,
            available_parallelism: std::thread::available_parallelism().ok().map(usize::from),
        },
        memory,
        gpus,
        disks,
        inference_status: "not_implemented",
        warnings,
    })
}

pub fn write_report(report: &DoctorReport, json: bool, output: &mut impl Write) -> io::Result<()> {
    if json {
        serde_json::to_writer_pretty(&mut *output, report).map_err(|error| {
            io::Error::new(error.io_error_kind().unwrap_or(io::ErrorKind::Other), error)
        })?;
        writeln!(output)?;
    } else {
        writeln!(
            output,
            "Lyra {} — локальная диагностика",
            report.lyra_version
        )?;
        writeln!(
            output,
            "ОС: {} ({})",
            report.os.as_deref().unwrap_or("недоступно"),
            report.architecture
        )?;
        writeln!(
            output,
            "CPU: {}",
            report.cpu.brand.as_deref().unwrap_or("недоступно")
        )?;
        writeln!(
            output,
            "Логические CPU: {}",
            optional_count(report.cpu.logical_cpus)
        )?;
        writeln!(
            output,
            "Доступный параллелизм: {}",
            optional_count(report.cpu.available_parallelism)
        )?;
        writeln!(
            output,
            "RAM: {}, доступно {}",
            gib(report.memory.total_bytes),
            gib(report.memory.available_bytes)
        )?;
        if let Some(gpus) = &report.gpus {
            for (index, gpu) in gpus.iter().enumerate() {
                writeln!(
                    output,
                    "GPU {}: {}, выделенная VRAM {}",
                    index + 1,
                    gpu.model.as_deref().unwrap_or("неизвестная модель"),
                    gib(gpu.dedicated_vram_bytes)
                )?;
            }
        }
        if let Some(disks) = &report.disks {
            for disk in disks {
                writeln!(
                    output,
                    "Диск {} ({}): доступно {} из {}",
                    disk.mount_point,
                    disk.kind,
                    gib(Some(disk.available_bytes)),
                    gib(Some(disk.total_bytes))
                )?;
            }
        }
        for warning in &report.warnings {
            writeln!(output, "Примечание: {warning}")?;
        }
        writeln!(
            output,
            "Локальная модель ещё не подключена. Это характеристики текущей среды запуска."
        )?;
    }
    output.flush()
}

fn optional_count(value: Option<usize>) -> String {
    value.map_or_else(|| "недоступно".into(), |count| count.to_string())
}

fn gib(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "недоступно".into(),
        |bytes| format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incomplete_report() -> DoctorReport {
        DoctorReport {
            schema_version: 1,
            lyra_version: "test",
            os: None,
            architecture: "x86_64",
            cpu: CpuReport {
                brand: None,
                logical_cpus: None,
                available_parallelism: None,
            },
            memory: MemoryReport {
                total_bytes: None,
                available_bytes: Some(0),
            },
            gpus: None,
            disks: Some(vec![DiskReport {
                mount_point: "C:\\Проекты с пробелами\\\"test\"".into(),
                kind: "Unknown".into(),
                total_bytes: 1024,
                available_bytes: 0,
            }]),
            inference_status: "not_implemented",
            warnings: vec!["Нет доступа".into()],
        }
    }

    #[test]
    fn json_preserves_unknown_vs_zero_and_unicode_paths() {
        let mut output = Vec::new();
        write_report(&incomplete_report(), true, &mut output).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert!(value["gpus"].is_null());
        assert!(value["memory"]["total_bytes"].is_null());
        assert_eq!(value["memory"]["available_bytes"], 0);
        assert_eq!(
            value["disks"][0]["mount_point"],
            "C:\\Проекты с пробелами\\\"test\""
        );
    }

    #[test]
    fn text_does_not_present_unknown_memory_as_zero() {
        let mut output = Vec::new();
        write_report(&incomplete_report(), false, &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("RAM: недоступно, доступно 0.00 GiB"));
        assert!(text.contains("Нет доступа"));
        assert!(text.contains("модель ещё не подключена"));
    }

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "output unavailable"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn output_errors_are_propagated() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::BrokenPipe] {
            for json in [false, true] {
                let error =
                    write_report(&incomplete_report(), json, &mut FailingWriter(kind)).unwrap_err();
                assert_eq!(error.kind(), kind);
            }
        }
    }
}
