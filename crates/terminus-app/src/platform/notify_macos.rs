//! Desktop notifications on macOS through `UNUserNotificationCenter`. The notification centre
//! exists only for a process that runs from an application bundle, so a bare binary (a development
//! build run from `target`) gets an error string instead of a crash.

use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
    UNUserNotificationCenter,
};

/// How long to wait for the user to answer the one time permission dialog.
const AUTHORISATION_WAIT: Duration = Duration::from_secs(120);
/// How long to wait for the centre to accept a request.
const ADD_WAIT: Duration = Duration::from_secs(10);

/// The outcome of the permission request, made once per process.
static AUTHORISED: OnceLock<Result<(), String>> = OnceLock::new();

/// True when this process runs from an application bundle, which is the only case that has a
/// notification centre.
fn in_bundle() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

/// The text of an `NSError` that Cocoa passed to a completion block, when there is one.
fn error_text(error: *mut NSError) -> Option<String> {
    // SAFETY: Cocoa passes either null or a valid NSError that lives for the block call.
    unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string())
}

/// Asks for permission to show alerts and play sounds, and waits for the answer.
fn request_authorisation(center: &UNUserNotificationCenter) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        let answer = match (granted.as_bool(), error_text(error)) {
            (true, _) => Ok(()),
            (false, Some(text)) => Err(format!("notifications are not allowed: {text}")),
            (false, None) => Err("notifications are turned off for Terminus in System Settings".to_string()),
        };
        let _ = tx.send(answer);
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &block,
    );
    rx.recv_timeout(AUTHORISATION_WAIT)
        .map_err(|_| "the notification permission dialog was not answered".to_string())?
}

/// Builds a request for a title and body that shows at once, with the default sound.
fn build_request(title: &str, body: &str) -> objc2::rc::Retained<UNNotificationRequest> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let identifier = NSString::from_str(&format!("terminus-{nanos}"));
    UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None)
}

/// Shows one notification. The permission is requested the first time only.
pub fn show(title: &str, body: &str) -> Result<(), String> {
    if !in_bundle() {
        return Err(
            "macOS shows notifications only for an application bundle, and this binary runs outside Terminus.app"
                .into(),
        );
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    AUTHORISED.get_or_init(|| request_authorisation(&center)).clone()?;
    let (tx, rx) = mpsc::channel();
    let done = RcBlock::new(move |error: *mut NSError| {
        let _ = tx.send(error_text(error));
    });
    center.addNotificationRequest_withCompletionHandler(&build_request(title, body), Some(&done));
    match rx.recv_timeout(ADD_WAIT) {
        Ok(None) => Ok(()),
        Ok(Some(text)) => Err(text),
        Err(_) => Err("the notification centre did not answer".into()),
    }
}
