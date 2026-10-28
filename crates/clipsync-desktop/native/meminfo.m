#include <mach/mach.h>

unsigned long long cs_memory_resident(void) {
    mach_task_basic_info_data_t info;
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t) &info, &count) != KERN_SUCCESS) {
        return 0;
    }
    return (unsigned long long) info.resident_size;
}
