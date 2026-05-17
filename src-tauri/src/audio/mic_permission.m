// ObjC shim that exposes AVCaptureDevice mic-permission APIs as plain C so
// the Rust side can FFI into them the same way it FFIs into AXIsProcessTrusted
// in pipeline.rs. Compiled by build.rs and linked into the lib.
//
// AVAuthorizationStatus enum values (stable, documented):
//   0 = notDetermined, 1 = restricted, 2 = denied, 3 = authorized

#import <AVFoundation/AVFoundation.h>

typedef void (*otl_mic_request_callback)(int /* granted */, void* /* ctx */);

int otl_mic_authorization_status(void) {
    return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

void otl_mic_request_access(otl_mic_request_callback cb, void* ctx) {
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                             completionHandler:^(BOOL granted) {
        cb(granted ? 1 : 0, ctx);
    }];
}
