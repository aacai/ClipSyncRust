#include <mach/mach.h>

// MACH_TASK_BASIC_INFO 的 resident_size 就是 top 的 RES：常驻物理页。三端统一按这个口径
// （Linux 的 VmRSS、Windows 的工作集是同一件事），不去凑活动监视器那一列的 phys_footprint
// ——那个把压缩前账、written-back 都算进去，跟另两端的数没法对着看。
unsigned long long cs_memory_resident(void) {
    mach_task_basic_info_data_t info;
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t) &info, &count) != KERN_SUCCESS) {
        return 0;
    }
    return (unsigned long long) info.resident_size;
}
