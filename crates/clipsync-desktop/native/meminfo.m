#include <mach/mach.h>

// 活动监视器「内存」那一列读的就是 phys_footprint（不是 resident_size：后者把压缩前后的
// 账混着算，跟系统监视器对不上）。自己问自己要 TASK_VM_INFO，不需要任何权限。
unsigned long long cs_memory_footprint(void) {
    task_vm_info_data_t info;
    mach_msg_type_number_t count = TASK_VM_INFO_COUNT;
    if (task_info(mach_task_self(), TASK_VM_INFO, (task_info_t) &info, &count) != KERN_SUCCESS) {
        return 0;
    }
    return (unsigned long long) info.phys_footprint;
}
