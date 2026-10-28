//! 本进程的常驻物理内存（RSS）：macOS 取 resident_size，Linux 读 VmRSS，Windows 取工作集。

pub fn resident() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn cs_memory_resident() -> u64;
        }
        let bytes = unsafe { cs_memory_resident() };
        (bytes > 0).then_some(bytes)
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb: u64 = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        (kb > 0).then_some(kb * 1024)
    }
    #[cfg(windows)]
    {
        use std::ffi::c_void;

        #[repr(C)]
        #[derive(Default)]
        struct Counters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set: usize,
            working_set: usize,
            quota_peak_paged_pool: usize,
            quota_paged_pool: usize,
            quota_peak_non_paged_pool: usize,
            quota_non_paged_pool: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn K32GetProcessMemoryInfo(
                process: *mut c_void,
                counters: *mut Counters,
                cb: u32,
            ) -> i32;
        }

        let size = std::mem::size_of::<Counters>() as u32;
        let mut counters = Counters {
            cb: size,
            ..Default::default()
        };
        let this_process = -1isize as *mut c_void;
        let ok = unsafe { K32GetProcessMemoryInfo(this_process, &mut counters, size) };
        (ok != 0 && counters.working_set > 0).then_some(counters.working_set as u64)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        windows
    )))]
    {
        None
    }
}

pub fn format(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes < MB {
        return format!("{} KB", bytes / KB);
    }
    if bytes < 10 * MB {
        return format!("{:.1} MB", bytes as f64 / MB as f64);
    }
    if bytes < GB {
        return format!("{} MB", bytes / MB);
    }
    format!("{:.1} GB", bytes as f64 / GB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_process_reports_its_own_resident_memory() {
        let bytes = resident().expect("this platform can ask for its own resident size");
        assert!(
            bytes > 1024 * 1024 && bytes < 4 * 1024 * 1024 * 1024,
            "implausible resident size {bytes}"
        );
    }

    #[test]
    fn sizes_switch_unit_where_the_reader_expects_it() {
        let kb = 1024_u64;
        let mb = kb * 1024;
        assert_eq!(format(512 * kb), "512 KB");
        assert_eq!(format(mb - 1), "1023 KB");
        assert_eq!(format(mb), "1.0 MB");
        assert_eq!(format(8 * mb + mb / 2), "8.5 MB");
        assert_eq!(format(10 * mb), "10 MB");
        assert_eq!(format(84 * mb + 400 * kb), "84 MB");
        assert_eq!(format(1023 * mb + mb / 2), "1023 MB");
        assert_eq!(format(2 * 1024 * mb), "2.0 GB");
    }
}
