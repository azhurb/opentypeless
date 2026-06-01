// ObjC shim implementing delayed (lazy) clipboard rendering so the Rust side
// can observe whether a paste actually landed. Compiled by build.rs and linked
// into the lib, exactly like src/audio/mic_permission.m exposes AVFoundation as
// plain C.
//
// The idea: instead of writing the dictation text to the pasteboard eagerly, we
// register an NSPasteboardItem whose data is produced ON DEMAND by this provider
// the moment a consumer (the app receiving our synthesized Cmd+V) reads it. If
// nobody reads within a short window, the paste had nowhere to land.
//
// Two pasteboard types are declared on the item:
//   * public.utf8-plain-text  — the real text; a genuine paste target reads this.
//   * com.opentypeless.dictation (private sentinel) — no real paste target asks
//     for an unknown private type, but a clipboard manager that mirrors the whole
//     pasteboard into history reads *every* type, including this one. A request
//     for the sentinel therefore flags a greedy background reader, letting the
//     Rust side avoid mistaking that read for a genuine paste and avoid
//     destroying the dictation by restoring the previous clipboard over it.
//
// The Rust side POLLS the consume flags (atomic properties) rather than taking a
// callback, which sidesteps any callback-lifetime hazard across the FFI boundary.

#import <AppKit/AppKit.h>

static NSString *const kOtlSentinelType = @"com.opentypeless.dictation";

// Strong references to providers still attached to the pasteboard. AppKit does
// not reliably keep the data provider alive, so we hold it here until the item
// leaves the pasteboard (pasteboardFinishedWithDataProvider:). Only ever mutated
// on the main thread.
static NSMutableSet *gActiveProviders;

@interface OtlDelayedPasteProvider : NSObject <NSPasteboardItemDataProvider>
@property(nonatomic, copy) NSString *text;
@property(atomic, assign) BOOL plainConsumed;
@property(atomic, assign) BOOL sentinelConsumed;
@end

@implementation OtlDelayedPasteProvider

- (void)pasteboard:(NSPasteboard *)pasteboard
                item:(NSPasteboardItem *)item
  provideDataForType:(NSPasteboardType)type {
    if ([type isEqualToString:kOtlSentinelType]) {
        self.sentinelConsumed = YES;
        // The sentinel exists only to be observed; hand back an empty payload.
        [item setData:[NSData data] forType:type];
    } else {
        self.plainConsumed = YES;
        // Materialize the real text on demand → the paste lands in the consumer.
        [item setString:(self.text ?: @"") forType:type];
    }
}

- (void)pasteboardFinishedWithDataProvider:(NSPasteboard *)pasteboard {
    // The item left the pasteboard (cleared/replaced or app exit); drop our
    // strong reference. Marshalled to the main thread so gActiveProviders is
    // only ever touched there.
    dispatch_async(dispatch_get_main_queue(), ^{
        [gActiveProviders removeObject:self];
    });
}

@end

// Write `utf8` to the general pasteboard lazily under the plain-text and sentinel
// types. Returns a +1 retained opaque handle (CFBridgingRetain) the caller MUST
// balance with otl_pasteboard_provider_release after the detection window — this
// guarantees the provider stays alive while Rust polls it even if the user copies
// something else mid-window. Returns NULL on failure. Call on the main thread.
void *otl_pasteboard_write_lazy(const char *utf8) {
    @autoreleasepool {
        NSString *s = utf8 ? [NSString stringWithUTF8String:utf8] : nil;
        if (s == nil) {
            return NULL;
        }
        OtlDelayedPasteProvider *provider = [[OtlDelayedPasteProvider alloc] init];
        provider.text = s;

        if (gActiveProviders == nil) {
            gActiveProviders = [NSMutableSet set];
        }
        [gActiveProviders addObject:provider];

        NSPasteboardItem *item = [[NSPasteboardItem alloc] init];
        [item setDataProvider:provider
                     forTypes:@[ NSPasteboardTypeString, kOtlSentinelType ]];

        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        [pb clearContents];
        if (![pb writeObjects:@[ item ]]) {
            [gActiveProviders removeObject:provider];
            return NULL;
        }
        return (void *)CFBridgingRetain(provider);
    }
}

// Returns 1 if the plain-text type has been consumed (the paste landed). When
// out_sentinel is non-NULL it receives 1 if the private sentinel type was read
// (a greedy background reader / clipboard manager is present). Safe to call from
// any thread while the handle is alive (the properties are atomic).
int otl_pasteboard_consumed(void *handle, int *out_sentinel) {
    if (handle == NULL) {
        if (out_sentinel) {
            *out_sentinel = 0;
        }
        return 0;
    }
    OtlDelayedPasteProvider *provider = (__bridge OtlDelayedPasteProvider *)handle;
    if (out_sentinel) {
        *out_sentinel = provider.sentinelConsumed ? 1 : 0;
    }
    return provider.plainConsumed ? 1 : 0;
}

// Replace the lazy pasteboard contents with a concrete plain-text item. Used on
// the not-landed (timeout) path so a later manual Cmd+V still finds the text even
// after the provider is released.
void otl_pasteboard_materialize(const char *utf8) {
    @autoreleasepool {
        NSString *s = utf8 ? [NSString stringWithUTF8String:utf8] : @"";
        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        [pb clearContents];
        [pb setString:(s ?: @"") forType:NSPasteboardTypeString];
    }
}

// Balance the CFBridgingRetain from otl_pasteboard_write_lazy. The provider may
// outlive this call (gActiveProviders keeps it alive until the item leaves the
// pasteboard), so this only drops the caller's extra reference.
void otl_pasteboard_provider_release(void *handle) {
    if (handle) {
        CFBridgingRelease(handle);
    }
}
