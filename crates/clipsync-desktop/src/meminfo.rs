//! 状态卡片上的「内存」：这个进程当前占了多少物理内存。
//!
//! 三端各拿各内核的那一个数，全部对齐「常驻物理页」这同一个口径：macOS 用
//! `mach_task_basic_info.resident_size`（top 的 RES），Linux 读 `/proc/self/status` 的
//! `VmRSS`，Windows 用 `K32GetProcessMemoryInfo` 的 `WorkingSetSize`（任务管理器「内存」
//! 那一列）。都不是「只算自家分配器」的托管堆口径——那种数会漏掉纹理、解码出来的图这些
//! 真正吃内存的东西。
//!
//! 特意没用 macOS 的 `phys_footprint`：那是活动监视器那一列，把压缩前的账和 written-back
//! 都算进去，跟另两端的数对不上看。
//!
//! 跟 `win_caption` 一样：所有平台都参与编译，`#[cfg]` 收在函数体内，调用点不写条件，
//! 免得又出现「本机全绿、别的平台编译不过」。Linux / Windows 两分支本机交叉不了编译，
//! 由 CI 那两个作业把关。

/// 当前进程占用的物理内存；拿不到就返回 `None`，界面上那一行直接不显示。
pub fn resident() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn cs_memory_resident() -> u64;
        }
        // 0 是 `task_info` 失败时的返回值，不是「真的一字节没占」。
        let bytes = unsafe { cs_memory_resident() };
        (bytes > 0).then_some(bytes)
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // VmRSS 是内核算好的「88420 kB」，省掉自己乘 page size——ARM 板上可能是 16K 页。
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

        /// PROCESS_MEMORY_COUNTERS：字段顺序照 Windows SDK，`usize` 对应 SIZE_T。
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
        // GetCurrentProcess() 是个宏，展开成 -1 这个伪句柄；查自己不用开句柄。
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

/// 两端显示同一个数就得同一套规则：1MB 以下报 KB，10MB 以下留一位小数，再往上取整到
/// MB，过 1GB 换单位，免得泄漏时甩出「2048 MB」。
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
        // 测试进程本身就不可能低于 1MB，也不可能大过 4GB：越界说明拿错了字段。
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
        // 10MB 起收掉小数，读数不跳字。
        assert_eq!(format(10 * mb), "10 MB");
        assert_eq!(format(84 * mb + 400 * kb), "84 MB");
        assert_eq!(format(1023 * mb + mb / 2), "1023 MB");
        assert_eq!(format(2 * 1024 * mb), "2.0 GB");
    }
}
