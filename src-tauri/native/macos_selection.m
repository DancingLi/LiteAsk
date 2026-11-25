#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#import <math.h>
#import <stdatomic.h>

typedef void (*LiteAskSelectionCallback)(const char *text, double x, double y, const char *app_id);
typedef void (*LiteAskDismissCallback)(double x, double y, bool is_click);
typedef bool (*LiteAskFilterCallback)(const char *app_id);
typedef void (*LiteAskReopenCallback)(void);

static LiteAskSelectionCallback g_on_selection = NULL;
static LiteAskDismissCallback g_on_dismiss = NULL;
static LiteAskFilterCallback g_is_blocked_app = NULL;
static LiteAskReopenCallback g_on_reopen = NULL;
static CFAbsoluteTime g_reopen_registered_at = 0;

@interface LiteAskAppleEventHandler : NSObject
- (void)handleReopenEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;
- (void)handleOpenAppEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent;
@end

@implementation LiteAskAppleEventHandler
- (void)handleReopenEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent {
    (void)event;
    (void)replyEvent;
    if (g_on_reopen) {
        g_on_reopen();
    }
}

- (void)handleOpenAppEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent {
    (void)event;
    (void)replyEvent;
    // Ignore the initial launch `oapp` event; treat later `oapp` events as user reopening the app.
    if (g_on_reopen && (CFAbsoluteTimeGetCurrent() - g_reopen_registered_at) > 1.2) {
        g_on_reopen();
    }
}
@end

static LiteAskAppleEventHandler *g_ae_handler = nil;

static CFMachPortRef g_event_tap = NULL;
static _Atomic bool g_enabled = true;
static _Atomic uint64_t g_generation = 0;

// Mouse gesture tracking (accessed only on event tap thread)
static BOOL g_mouse_down = NO;
static CGPoint g_down_point = {0, 0};
static CFAbsoluteTime g_down_time = 0;
static CFAbsoluteTime g_last_up_time = 0;
static CGPoint g_last_up_point = {0, 0};
static int g_click_streak = 0;

static dispatch_queue_t g_extract_queue = NULL;

bool liteask_mac_is_accessibility_trusted(bool prompt) {
    @autoreleasepool {
        NSDictionary *options = @{(__bridge id)kAXTrustedCheckOptionPrompt: @(prompt)};
        return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
    }
}

void liteask_mac_open_accessibility_settings(void) {
    @autoreleasepool {
        NSURL *url = [NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"];
        if (url) {
            [[NSWorkspace sharedWorkspace] openURL:url];
        }
    }
}

void liteask_mac_set_selection_enabled(bool enabled) {
    atomic_store(&g_enabled, enabled);
    if (!enabled) {
        atomic_fetch_add(&g_generation, 1);
    }
}

void liteask_mac_configure_floating_panel(void *ns_window_ptr, bool non_activating) {
    if (!ns_window_ptr) return;
    NSWindow *window = (__bridge NSWindow *)ns_window_ptr;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSWindowCollectionBehavior behavior =
            NSWindowCollectionBehaviorCanJoinAllSpaces |
            NSWindowCollectionBehaviorFullScreenAuxiliary |
            NSWindowCollectionBehaviorStationary |
            NSWindowCollectionBehaviorIgnoresCycle;
        [window setCollectionBehavior:behavior];
        [window setLevel:NSStatusWindowLevel];
        [window setHidesOnDeactivate:NO];
        if (non_activating) {
            [window setStyleMask:([window styleMask] | NSWindowStyleMaskNonactivatingPanel)];
        }
    });
}

void liteask_mac_show_inactive(void *ns_window_ptr) {
    if (!ns_window_ptr) return;
    NSWindow *window = (__bridge NSWindow *)ns_window_ptr;
    dispatch_async(dispatch_get_main_queue(), ^{
        [window setLevel:NSStatusWindowLevel];
        [window orderFrontRegardless];
    });
}

void liteask_mac_install_reopen_handler(LiteAskReopenCallback on_reopen) {
    g_on_reopen = on_reopen;
    g_reopen_registered_at = CFAbsoluteTimeGetCurrent();
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!g_ae_handler) {
            g_ae_handler = [[LiteAskAppleEventHandler alloc] init];
        }
        NSAppleEventManager *mgr = [NSAppleEventManager sharedAppleEventManager];
        [mgr setEventHandler:g_ae_handler
                 andSelector:@selector(handleReopenEvent:withReplyEvent:)
               forEventClass:'aevt'
                  andEventID:'rapp'];
        [mgr setEventHandler:g_ae_handler
                 andSelector:@selector(handleOpenAppEvent:withReplyEvent:)
               forEventClass:'aevt'
                  andEventID:'oapp'];
    });
}

void liteask_mac_activate_and_focus(void *ns_window_ptr, bool floating) {
    if (!ns_window_ptr) return;
    NSWindow *window = (__bridge NSWindow *)ns_window_ptr;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (floating) {
            [window setLevel:NSFloatingWindowLevel];
        } else {
            [window setLevel:NSNormalWindowLevel];
            [window setCollectionBehavior:([window collectionBehavior] | NSWindowCollectionBehaviorMoveToActiveSpace)];
        }
        if ([window isMiniaturized]) {
            [window deminiaturize:nil];
        }
        if (@available(macOS 14.0, *)) {
            [NSApp activate];
        }
        [NSApp activateIgnoringOtherApps:YES];
        [window makeKeyAndOrderFront:nil];
        if (floating) {
            [window orderFrontRegardless];
        }
    });
}

static NSString *get_selected_text_via_ax(void) {
    AXUIElementRef systemWide = AXUIElementCreateSystemWide();
    if (!systemWide) return nil;

    CFTypeRef focusedObj = NULL;
    AXError err = AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute, &focusedObj);
    CFRelease(systemWide);

    if (err != kAXErrorSuccess || !focusedObj) {
        return nil;
    }

    AXUIElementRef focusedElement = (AXUIElementRef)focusedObj;
    CFTypeRef selectedTextObj = NULL;
    err = AXUIElementCopyAttributeValue(focusedElement, kAXSelectedTextAttribute, &selectedTextObj);

    NSString *result = nil;
    if (err == kAXErrorSuccess && selectedTextObj) {
        if (CFGetTypeID(selectedTextObj) == CFStringGetTypeID()) {
            result = [(__bridge NSString *)selectedTextObj copy];
        }
        CFRelease(selectedTextObj);
    }

    // Fallback: AXSelectedTextRange + AXStringForRangeParameterizedAttribute (helps WebKit/some rich views)
    if (!result || result.length == 0) {
        CFTypeRef rangeObj = NULL;
        if (AXUIElementCopyAttributeValue(focusedElement, kAXSelectedTextRangeAttribute, &rangeObj) == kAXErrorSuccess && rangeObj) {
            CFTypeRef rangedText = NULL;
            if (AXUIElementCopyParameterizedAttributeValue(focusedElement, kAXStringForRangeParameterizedAttribute, rangeObj, &rangedText) == kAXErrorSuccess && rangedText) {
                if (CFGetTypeID(rangedText) == CFStringGetTypeID()) {
                    result = [(__bridge NSString *)rangedText copy];
                }
                CFRelease(rangedText);
            }
            CFRelease(rangeObj);
        }
    }

    CFRelease(focusedElement);
    return result;
}

static NSArray<NSDictionary<NSPasteboardType, NSData *> *> *snapshot_pasteboard(NSPasteboard *pb) {
    NSMutableArray *backup = [NSMutableArray array];
    for (NSPasteboardItem *item in pb.pasteboardItems) {
        NSMutableDictionary<NSPasteboardType, NSData *> *entry = [NSMutableDictionary dictionary];
        for (NSPasteboardType type in item.types) {
            NSData *data = [item dataForType:type];
            if (data) {
                entry[type] = [data copy];
            }
        }
        if (entry.count > 0) {
            [backup addObject:entry];
        }
    }
    return backup;
}

static void restore_pasteboard(NSPasteboard *pb, NSArray<NSDictionary<NSPasteboardType, NSData *> *> *backup) {
    [pb clearContents];
    if (backup.count == 0) return;

    NSMutableArray<NSPasteboardItem *> *items = [NSMutableArray arrayWithCapacity:backup.count];
    for (NSDictionary<NSPasteboardType, NSData *> *entry in backup) {
        NSPasteboardItem *item = [[NSPasteboardItem alloc] init];
        [entry enumerateKeysAndObjectsUsingBlock:^(NSPasteboardType type, NSData *data, BOOL *stop) {
            (void)stop;
            [item setData:data forType:type];
        }];
        [items addObject:item];
    }
    [pb writeObjects:items];
}

static void post_cmd_c(void) {
    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    // Keycode 8 = 'c' on US/standard macOS layout
    CGEventRef keyDown = CGEventCreateKeyboardEvent(source, (CGKeyCode)8, true);
    CGEventRef keyUp = CGEventCreateKeyboardEvent(source, (CGKeyCode)8, false);
    if (keyDown && keyUp) {
        CGEventSetFlags(keyDown, kCGEventFlagMaskCommand);
        CGEventSetFlags(keyUp, kCGEventFlagMaskCommand);
        CGEventPost(kCGHIDEventTap, keyDown);
        usleep(12000); // 12ms
        CGEventPost(kCGHIDEventTap, keyUp);
    }
    if (keyDown) CFRelease(keyDown);
    if (keyUp) CFRelease(keyUp);
    if (source) CFRelease(source);
}

static NSString *get_selected_text_via_clipboard(uint64_t expected_gen) {
    NSPasteboard *pb = [NSPasteboard generalPasteboard];
    NSArray *backup = snapshot_pasteboard(pb);
    NSInteger beforeCount = pb.changeCount;

    post_cmd_c();

    // Poll up to ~150ms for pasteboard update
    BOOL changed = NO;
    for (int i = 0; i < 10; i++) {
        usleep(15000); // 15ms
        if (atomic_load(&g_generation) != expected_gen) {
            // Another interaction superseded this one; restore if needed and bail
            if (pb.changeCount != beforeCount) {
                restore_pasteboard(pb, backup);
            }
            return nil;
        }
        if (pb.changeCount != beforeCount) {
            changed = YES;
            break;
        }
    }

    if (!changed) {
        return nil;
    }

    NSString *copied = [pb stringForType:NSPasteboardTypeString];
    NSString *result = copied ? [copied copy] : nil;

    // Immediately restore original clipboard contents so user's clipboard is untouched
    restore_pasteboard(pb, backup);
    return result;
}

static void schedule_text_extraction(CGPoint releasePoint) {
    uint64_t gen = atomic_fetch_add(&g_generation, 1) + 1;
    if (!g_extract_queue) return;

    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(55 * NSEC_PER_MSEC)), g_extract_queue, ^{
        @autoreleasepool {
            if (!atomic_load(&g_enabled)) return;
            if (atomic_load(&g_generation) != gen) return;

            NSRunningApplication *frontApp = [[NSWorkspace sharedWorkspace] frontmostApplication];
            pid_t selfPid = [[NSProcessInfo processInfo] processIdentifier];
            if (frontApp && frontApp.processIdentifier == selfPid) {
                return;
            }

            NSString *bundleId = frontApp.bundleIdentifier ?: @"";
            const char *bundleCStr = [bundleId UTF8String] ?: "";

            if (g_is_blocked_app && g_is_blocked_app(bundleCStr)) {
                return;
            }

            // Stage 1: Instant zero-clipboard Accessibility read
            NSString *text = get_selected_text_via_ax();

            // Stage 2: Safe clipboard backup/copy/restore fallback for Chrome/Electron/PDF
            NSString *trimmed = [text stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
            if (!trimmed || trimmed.length == 0) {
                text = get_selected_text_via_clipboard(gen);
                trimmed = [text stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
            }

            if (atomic_load(&g_generation) != gen) return;
            if (!trimmed || trimmed.length == 0) return;
            // Ignore absurdly huge accidental selections (> 15k chars)
            if (trimmed.length > 15000) {
                trimmed = [trimmed substringToIndex:15000];
            }

            if (g_on_selection) {
                const char *utf8 = [trimmed UTF8String];
                if (utf8) {
                    g_on_selection(utf8, releasePoint.x, releasePoint.y, bundleCStr);
                }
            }
        }
    });
}

static CGEventRef event_tap_callback(CGEventTapProxy proxy, CGEventType type, CGEventRef event, void *refcon) {
    (void)proxy;
    (void)refcon;

    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (g_event_tap) {
            CGEventTapEnable(g_event_tap, true);
        }
        return event;
    }

    if (!atomic_load(&g_enabled)) {
        return event;
    }

    CGPoint loc = CGEventGetLocation(event);

    switch (type) {
        case kCGEventLeftMouseDown: {
            g_mouse_down = YES;
            g_down_point = loc;
            g_down_time = CFAbsoluteTimeGetCurrent();
            atomic_fetch_add(&g_generation, 1);

            if (g_on_dismiss) {
                g_on_dismiss(loc.x, loc.y, true);
            }
            break;
        }
        case kCGEventLeftMouseUp: {
            if (!g_mouse_down) break;
            g_mouse_down = NO;

            CFAbsoluteTime now = CFAbsoluteTimeGetCurrent();
            double dx = loc.x - g_down_point.x;
            double dy = loc.y - g_down_point.y;
            double dragDist = hypot(dx, dy);

            double sinceLastUp = now - g_last_up_time;
            double distFromLastUp = hypot(loc.x - g_last_up_point.x, loc.y - g_last_up_point.y);

            if (sinceLastUp <= 0.45 && distFromLastUp <= 8.0) {
                g_click_streak += 1;
            } else {
                g_click_streak = 1;
            }
            g_last_up_time = now;
            g_last_up_point = loc;

            // Trigger on genuine drag selection (>= 6px) OR double/triple click word/line selection
            BOOL isDragSelection = (dragDist >= 6.0 && (now - g_down_time) < 10.0);
            BOOL isMultiClickSelection = (g_click_streak >= 2 && dragDist < 6.0);

            if (isDragSelection || isMultiClickSelection) {
                schedule_text_extraction(loc);
            }
            break;
        }
        case kCGEventRightMouseDown:
        case kCGEventScrollWheel: {
            atomic_fetch_add(&g_generation, 1);
            if (g_on_dismiss) {
                g_on_dismiss(loc.x, loc.y, false);
            }
            break;
        }
        case kCGEventKeyDown: {
            // Dismiss toolbar on key press (ignore our own synthetic Cmd+C by checking flag)
            CGEventFlags flags = CGEventGetFlags(event);
            int64_t keycode = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
            BOOL isSyntheticCmdC = (keycode == 8 && (flags & kCGEventFlagMaskCommand));
            if (!isSyntheticCmdC && g_on_dismiss) {
                g_on_dismiss(loc.x, loc.y, false);
            }
            break;
        }
        default:
            break;
    }

    return event;
}

bool liteask_mac_start_selection_monitor(
    LiteAskSelectionCallback on_selection,
    LiteAskDismissCallback on_dismiss,
    LiteAskFilterCallback is_blocked_app
) {
    g_on_selection = on_selection;
    g_on_dismiss = on_dismiss;
    g_is_blocked_app = is_blocked_app;

    if (!g_extract_queue) {
        g_extract_queue = dispatch_queue_create("com.liteask.selection.extract", DISPATCH_QUEUE_SERIAL);
    }

    if (g_event_tap != NULL) {
        CGEventTapEnable(g_event_tap, true);
        return true;
    }

    if (!liteask_mac_is_accessibility_trusted(false)) {
        return false;
    }

    CGEventMask mask =
        CGEventMaskBit(kCGEventLeftMouseDown) |
        CGEventMaskBit(kCGEventLeftMouseUp) |
        CGEventMaskBit(kCGEventRightMouseDown) |
        CGEventMaskBit(kCGEventScrollWheel) |
        CGEventMaskBit(kCGEventKeyDown);

    g_event_tap = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionListenOnly,
        mask,
        event_tap_callback,
        NULL
    );

    if (!g_event_tap) {
        return false;
    }

    CFRunLoopSourceRef source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, g_event_tap, 0);
    if (!source) {
        CFRelease(g_event_tap);
        g_event_tap = NULL;
        return false;
    }

    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0), ^{
        CFRunLoopRef runLoop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(runLoop, source, kCFRunLoopCommonModes);
        CGEventTapEnable(g_event_tap, true);
        CFRelease(source);
        CFRunLoopRun();
    });

    return true;
}
