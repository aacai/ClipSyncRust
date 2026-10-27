#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>

#include <pthread.h>
#include <string.h>

/// C API shared by the Compose dylib and the Rust static link.
/// Rect is in points, origin at the top-left of the main display. w<=1 or h<=1 means the whole display.
/// `mic_device` 是 AVCaptureDevice.uniqueID；空字符串表示系统默认输入。
int cs_record_start(double x, double y, double w, double h, int mic, int system_audio, int fps, int long_edge, const char *mic_device, const char *path);
int cs_record_stop(void);
int cs_record_active(void);
int cs_record_exclude_self(void);
/// 本机可用的麦克风，JSON 数组：[{"id":"…","name":"…"}]。读不到就是 []。
const char *cs_record_inputs(void);
const char *cs_record_last_error(void);

static char g_error[512];
static char g_inputs[8192];
static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;

static void set_error(NSString *text) {
    pthread_mutex_lock(&g_lock);
    const char *utf = text.UTF8String ?: "";
    strncpy(g_error, utf, sizeof(g_error) - 1);
    g_error[sizeof(g_error) - 1] = 0;
    pthread_mutex_unlock(&g_lock);
}

static void pump(NSDate *until) {
    [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:until];
}

static void wait_sem(dispatch_semaphore_t sem) {
    if (![NSThread isMainThread]) {
        dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
        return;
    }
    while (dispatch_semaphore_wait(sem, DISPATCH_TIME_NOW) != 0) {
        pump([NSDate dateWithTimeIntervalSinceNow:0.02]);
    }
}

static BOOL wait_sem_timeout(dispatch_semaphore_t sem, double seconds) {
    if (![NSThread isMainThread]) {
        return dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(seconds * NSEC_PER_SEC))) == 0;
    }
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (dispatch_semaphore_wait(sem, DISPATCH_TIME_NOW) != 0) {
        if ([deadline timeIntervalSinceNow] <= 0) return NO;
        pump([NSDate dateWithTimeIntervalSinceNow:0.02]);
    }
    return YES;
}

/// 工作线程等不到回调也得有个交代，别让调用方永远卡在那里假装在录。
static BOOL wait_or_fail(dispatch_semaphore_t sem, NSString *what) {
    if (wait_sem_timeout(sem, 20.0)) return YES;
    set_error([NSString stringWithFormat:@"%@超时", what]);
    return NO;
}

@class CsRecorder;

static CsRecorder *g_recorder;

@interface CsRecorder : NSObject <SCStreamDelegate, SCRecordingOutputDelegate>
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic, strong) SCRecordingOutput *output;
@property(nonatomic, strong) dispatch_semaphore_t finished;
@property(nonatomic, assign) BOOL running;
@property(nonatomic, assign) BOOL failed;
- (void)releaseIfStopped;
@end

@implementation CsRecorder

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    (void)stream;
    if (error != nil) {
        self.failed = YES;
        set_error(error.localizedDescription);
    }
    if (self.finished != nil) dispatch_semaphore_signal(self.finished);
}

- (void)recordingOutputDidStartRecording:(SCRecordingOutput *)recordingOutput {
    (void)recordingOutput;
}

- (void)recordingOutputDidFinishRecording:(SCRecordingOutput *)recordingOutput {
    (void)recordingOutput;
    if (self.finished != nil) dispatch_semaphore_signal(self.finished);
    [self releaseIfStopped];
}

- (void)recordingOutput:(SCRecordingOutput *)recordingOutput didFailWithError:(NSError *)error {
    (void)recordingOutput;
    self.failed = YES;
    set_error(error.localizedDescription ?: @"录制失败");
    if (self.finished != nil) dispatch_semaphore_signal(self.finished);
    [self releaseIfStopped];
}

/// 收尾回调可能比 stop 更晚到；那时候才放手，别在文件还没写完时把 stream 释放掉。
- (void)releaseIfStopped {
    if (self.running) return;
    if (g_recorder == self) g_recorder = nil;
}

@end

static BOOL ensure_mic(void) {
    AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if (status == AVAuthorizationStatusAuthorized) return YES;
    if (status == AVAuthorizationStatusDenied || status == AVAuthorizationStatusRestricted) {
        set_error(@"没有麦克风权限：系统设置 → 隐私与安全性 → 麦克风，勾上本应用");
        return NO;
    }
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    __block BOOL granted = NO;
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                             completionHandler:^(BOOL ok) {
                                 granted = ok;
                                 dispatch_semaphore_signal(sem);
                             }];
    wait_sem(sem);
    if (!granted) set_error(@"没有麦克风权限：系统设置 → 隐私与安全性 → 麦克风，勾上本应用");
    return granted;
}

static size_t even_size(double points, CGFloat scale) {
    size_t px = (size_t)llround(points * scale);
    if (px < 2) px = 2;
    if (px % 2) px += 1;
    return px;
}

/// 主屏每点对应多少原生像素。不能用 NSScreen.mainScreen：开录这条路走在工作线程上，
/// 工作线程碰 AppKit 会直接 abort。
static CGFloat main_scale(void) {
    CGDirectDisplayID display = CGMainDisplayID();
    CGFloat points = CGDisplayBounds(display).size.width;
    size_t pixels = CGDisplayPixelsWide(display);
    if (points < 1.0 || pixels < 2) return 2.0;
    CGFloat scale = (CGFloat)pixels / points;
    return scale < 1.0 ? 1.0 : scale;
}

static void apply_output_size(SCStreamConfiguration *config, size_t width, size_t height, int long_edge) {
    if (long_edge > 0) {
        size_t long_side = width > height ? width : height;
        if (long_side > (size_t)long_edge) {
            double ratio = (double)long_edge / (double)long_side;
            width = (size_t)llround((double)width * ratio);
            height = (size_t)llround((double)height * ratio);
        }
    }
    if (width < 2) width = 2;
    if (height < 2) height = 2;
    if (width % 2) width += 1;
    if (height % 2) height += 1;
    config.width = width;
    config.height = height;
}

int cs_record_start(double x, double y, double w, double h, int mic, int system_audio, int fps, int long_edge, const char *mic_device, const char *path) {
    if (path == NULL || path[0] == 0) {
        set_error(@"没有输出路径");
        return -1;
    }
    if (g_recorder.running) return 1;
    @autoreleasepool {
        if (mic && !ensure_mic()) return -1;
        NSString *outPath = [NSString stringWithUTF8String:path];
        [[NSFileManager defaultManager] removeItemAtPath:outPath error:nil];
        NSURL *url = [NSURL fileURLWithPath:outPath];

        dispatch_semaphore_t sem = dispatch_semaphore_create(0);
        __block SCShareableContent *content = nil;
        __block NSError *contentError = nil;
        [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *c, NSError *e) {
            content = c;
            contentError = e;
            dispatch_semaphore_signal(sem);
        }];
        if (!wait_or_fail(sem, @"获取可录内容")) return -1;
        if (content == nil) {
            set_error(contentError.localizedDescription
                          ?: @"缺少屏幕录制权限：系统设置 → 隐私与安全性 → 屏幕与系统音频录制");
            return -1;
        }

        CGDirectDisplayID mainId = CGMainDisplayID();
        SCDisplay *display = nil;
        for (SCDisplay *candidate in content.displays) {
            if (candidate.displayID == mainId) {
                display = candidate;
                break;
            }
        }
        if (display == nil) display = content.displays.firstObject;
        if (display == nil) {
            set_error(@"没有可用的显示器");
            return -1;
        }

        pid_t selfPid = getpid();
        NSMutableArray<SCWindow *> *excluded = [NSMutableArray array];
        for (SCWindow *window in content.windows) {
            if (window.owningApplication.processID == selfPid) [excluded addObject:window];
        }
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:excluded];

        CGFloat scale = main_scale();
        BOOL region = w > 1.0 && h > 1.0;
        SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
        config.capturesAudio = system_audio ? YES : NO;
        config.captureMicrophone = mic ? YES : NO;
        // 指定输入设备（macOS 15 起 ScreenCaptureKit 才让挑）；空串沿用系统默认输入。
        if (@available(macOS 15.0, *)) {
            if (mic && mic_device != NULL && mic_device[0] != 0) {
                config.microphoneCaptureDeviceID = [NSString stringWithUTF8String:mic_device];
            }
        }
        config.excludesCurrentProcessAudio = YES;
        config.showsCursor = YES;
        int rate = fps;
        if (rate != 15 && rate != 30 && rate != 60) rate = 30;
        config.minimumFrameInterval = CMTimeMake(1, rate);
        config.queueDepth = 6;
        config.pixelFormat = kCVPixelFormatType_32BGRA;
        if (region) {
            config.sourceRect = CGRectMake(x, y, w, h);
            apply_output_size(config, even_size(w, scale), even_size(h, scale), long_edge);
        } else {
            apply_output_size(config, even_size(display.width, scale), even_size(display.height, scale), long_edge);
        }

        CsRecorder *recorder = [[CsRecorder alloc] init];
        SCStream *stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:recorder];
        SCRecordingOutputConfiguration *out = [[SCRecordingOutputConfiguration alloc] init];
        out.outputURL = url;
        out.outputFileType = AVFileTypeMPEG4;
        out.videoCodecType = AVVideoCodecTypeH264;
        // 系统声音与麦克风本来就是混成一条轨（mixesAudioWithMicrophone 默认 YES，且要 macOS 27 才有这个属性）
        SCRecordingOutput *output = [[SCRecordingOutput alloc] initWithConfiguration:out delegate:recorder];
        NSError *addError = nil;
        if (![stream addRecordingOutput:output error:&addError]) {
            set_error(addError.localizedDescription ?: @"无法开始写入录像");
            return -1;
        }

        dispatch_semaphore_t started = dispatch_semaphore_create(0);
        __block NSError *startError = nil;
        [stream startCaptureWithCompletionHandler:^(NSError *error) {
            startError = error;
            dispatch_semaphore_signal(started);
        }];
        if (!wait_or_fail(started, @"启动录屏")) {
            [stream stopCaptureWithCompletionHandler:^(NSError *error) { (void)error; }];
            return -1;
        }
        if (startError != nil) {
            set_error(startError.localizedDescription ?: @"无法开始录屏");
            return -1;
        }
        recorder.stream = stream;
        recorder.output = output;
        recorder.running = YES;
        g_recorder = recorder;
        set_error(@"");
        return 0;
    }
}

int cs_record_stop(void) {
    CsRecorder *recorder = g_recorder;
    if (recorder == nil || !recorder.running) return 0;
    @autoreleasepool {
        recorder.running = NO;
        recorder.finished = dispatch_semaphore_create(0);
        __block NSError *stopError = nil;
        dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
        [recorder.stream stopCaptureWithCompletionHandler:^(NSError *error) {
            stopError = error;
            dispatch_semaphore_signal(stopped);
        }];
        if (!wait_or_fail(stopped, @"停止录屏")) return -1;
        if (stopError != nil) {
            set_error(stopError.localizedDescription);
            return -1;
        }
        // 写 moov 比 stop 回调更晚，等不到就明确报失败，别把半成品当成品。
        if (!wait_sem_timeout(recorder.finished, 10.0)) {
            set_error(@"录像收尾超时，文件可能不完整");
            return -1;
        }
        return recorder.failed ? -1 : 0;
    }
}

int cs_record_active(void) {
    return g_recorder.running ? 1 : 0;
}

/// 列本机麦克风：给设置页挑输入设备用。返回静态缓冲区，调用方要立刻拷走。
const char *cs_record_inputs(void) {
    @autoreleasepool {
        NSMutableArray<AVCaptureDeviceType> *types = [NSMutableArray array];
        if (@available(macOS 14.0, *)) {
            [types addObject:AVCaptureDeviceTypeMicrophone];
            [types addObject:AVCaptureDeviceTypeExternal];
        } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            [types addObject:AVCaptureDeviceTypeBuiltInMicrophone];
            [types addObject:AVCaptureDeviceTypeExternalUnknown];
#pragma clang diagnostic pop
        }
        AVCaptureDeviceDiscoverySession *session = [AVCaptureDeviceDiscoverySession
            discoverySessionWithDeviceTypes:types
                                  mediaType:AVMediaTypeAudio
                                   position:AVCaptureDevicePositionUnspecified];
        NSMutableArray *list = [NSMutableArray array];
        for (AVCaptureDevice *device in session.devices) {
            NSString *uid = device.uniqueID ?: @"";
            NSString *name = device.localizedName ?: @"";
            [list addObject:@{@"id": uid, @"name": name}];
        }
        NSData *data = [NSJSONSerialization dataWithJSONObject:list options:0 error:nil];
        NSString *text = data != nil ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] : @"[]";
        pthread_mutex_lock(&g_lock);
        strncpy(g_inputs, (text ?: @"[]").UTF8String ?: "[]", sizeof(g_inputs) - 1);
        g_inputs[sizeof(g_inputs) - 1] = 0;
        pthread_mutex_unlock(&g_lock);
        return g_inputs;
    }
}

/// 录制药丸是开录之后才建出来的窗口，开录时的排除快照里没有它。
/// 重取一次可共享内容、把自己所有窗口再排一遍，写回 stream 的内容过滤器。
/// 必须带上隐藏窗口（onScreenWindowsOnly:NO），药丸在刷新这一刻还是藏着的。
int cs_record_exclude_self(void) {
    CsRecorder *recorder = g_recorder;
    if (recorder == nil || !recorder.running) return 0;
    dispatch_semaphore_t fetched = dispatch_semaphore_create(0);
    __block SCContentFilter *fresh = nil;
    __block NSError *fetchError = nil;
    [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                              onScreenWindowsOnly:NO
                                                completionHandler:^(SCShareableContent *content, NSError *error) {
        fetchError = error;
        if (content != nil) {
            pid_t selfPid = getpid();
            NSMutableArray<SCWindow *> *excluded = [NSMutableArray array];
            for (SCWindow *window in content.windows) {
                if (window.owningApplication.processID == selfPid) [excluded addObject:window];
            }
            SCDisplay *display = nil;
            CGDirectDisplayID mainId = CGMainDisplayID();
            for (SCDisplay *candidate in content.displays) {
                if (candidate.displayID == mainId) {
                    display = candidate;
                    break;
                }
            }
            if (display == nil) display = content.displays.firstObject;
            if (display != nil) {
                fresh = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:excluded];
            }
        }
        dispatch_semaphore_signal(fetched);
    }];
    if (!wait_or_fail(fetched, @"刷新录屏范围")) return -1;
    if (fresh == nil) {
        set_error(fetchError.localizedDescription ?: @"无法刷新录屏范围");
        return -1;
    }
    dispatch_semaphore_t updated = dispatch_semaphore_create(0);
    __block BOOL ok = NO;
    [recorder.stream updateContentFilter:fresh completionHandler:^(NSError *error) {
        if (error != nil) set_error(error.localizedDescription);
        ok = (error == nil);
        dispatch_semaphore_signal(updated);
    }];
    if (!wait_or_fail(updated, @"更新录屏范围")) return -1;
    return ok ? 0 : -1;
}

const char *cs_record_last_error(void) {
    return g_error;
}
