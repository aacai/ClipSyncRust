#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>

#include <string.h>
#include <unistd.h>

/// 普通层（layer 0）窗口的全局 bounds，点为单位、左上原点、顶层在前。
/// out 每条 4 个 double：x, y, w, h。返回写入的窗口个数。
int cs_snap_windows(double *out, int max) {
    if (out == NULL || max <= 0) return 0;
    CFArrayRef list = CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
    if (list == NULL) return 0;
    int count = 0;
    pid_t self = getpid();
    CFIndex total = CFArrayGetCount(list);
    for (CFIndex i = 0; i < total && count < max; i++) {
        CFDictionaryRef info = CFArrayGetValueAtIndex(list, i);
        if (info == NULL) continue;
        // 程序坞、菜单栏、通知中心这些都在 0 以外的层上，整屏大小会挡住所有应用窗口。
        int layer = -1;
        CFNumberRef layerNumber = CFDictionaryGetValue(info, kCGWindowLayer);
        if (layerNumber == NULL) continue;
        CFNumberGetValue(layerNumber, kCFNumberIntType, &layer);
        if (layer != 0) continue;
        int pid = 0;
        CFNumberRef pidNumber = CFDictionaryGetValue(info, kCGWindowOwnerPID);
        if (pidNumber != NULL) CFNumberGetValue(pidNumber, kCFNumberIntType, &pid);
        // 自己的窗口（冻结帧遮罩、录屏药丸）不能当目标。
        if (pid == self) continue;
        CGRect bounds;
        CFDictionaryRef boundsDictionary = CFDictionaryGetValue(info, kCGWindowBounds);
        if (boundsDictionary == NULL) continue;
        if (!CGRectMakeWithDictionaryRepresentation(boundsDictionary, &bounds)) continue;
        if (bounds.size.width < 40 || bounds.size.height < 40) continue;
        out[count * 4 + 0] = bounds.origin.x;
        out[count * 4 + 1] = bounds.origin.y;
        out[count * 4 + 2] = bounds.size.width;
        out[count * 4 + 3] = bounds.size.height;
        count++;
    }
    CFRelease(list);
    return count;
}
