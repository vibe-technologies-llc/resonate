use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc, Once,
        atomic::{AtomicU64, Ordering},
    },
};

use block2::{DynBlock, RcBlock};
use objc2::{
    AnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString, NSURL};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationAttachment, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use parking_lot::Mutex;

use crate::{Error, Result};

const TRANSPORT_CATEGORY: &str = "resonate.transport";
const THREAD: &str = "resonate";

type Answer = Arc<dyn Fn(&str) + Send + Sync>;

static ANSWERING: Mutex<BTreeMap<u64, Answer>> = Mutex::new(BTreeMap::new());
static NEXT_ASKER: AtomicU64 = AtomicU64::new(1);
static NEXT_PICTURE: AtomicU64 = AtomicU64::new(1);
static ANSWERED_BY_US: Once = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Button<'a> {
    pub key: &'a str,
    pub label: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub picture: Option<&'a Path>,
    pub offers_buttons: bool,
}

pub struct Notices {
    asker: u64,
}

impl Notices {
    pub fn open(
        buttons: &[Button<'_>],
        pressed: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<Self> {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return Err(Error::NoBundle);
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();

        ANSWERED_BY_US.call_once(|| answer_through(&center));
        center
            .setNotificationCategories(&NSSet::from_retained_slice(&[transport_category(buttons)]));
        let asked = RcBlock::new(|_granted: Bool, _refusal: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &asked,
        );

        let asker = NEXT_ASKER.fetch_add(1, Ordering::Relaxed);
        ANSWERING.lock().insert(asker, Arc::new(pressed));

        Ok(Self { asker })
    }

    pub fn raise(&self, kind: &str, notice: &Notice<'_>) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(notice.title));
        content.setBody(&NSString::from_str(notice.body));
        content.setThreadIdentifier(&NSString::from_str(THREAD));
        if notice.offers_buttons {
            content.setCategoryIdentifier(&NSString::from_str(TRANSPORT_CATEGORY));
        }
        if let Some(attachment) = notice.picture.and_then(attachment_of) {
            content.setAttachments(&NSArray::from_retained_slice(&[attachment]));
        }

        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&self.identifier(kind)),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    }

    pub fn hush(&self, kind: &str) {
        UNUserNotificationCenter::currentNotificationCenter()
            .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_retained_slice(&[
                NSString::from_str(&self.identifier(kind)),
            ]));
    }

    fn identifier(&self, kind: &str) -> String {
        identifier_of(self.asker, kind)
    }
}

impl Drop for Notices {
    fn drop(&mut self) {
        ANSWERING.lock().remove(&self.asker);
    }
}

fn identifier_of(asker: u64, kind: &str) -> String {
    format!("{asker}.{kind}")
}

fn asker_of(identifier: &str) -> Option<u64> {
    identifier.split_once('.')?.0.parse().ok()
}

fn answered(identifier: &str, key: &str) {
    let answer = asker_of(identifier).and_then(|asker| ANSWERING.lock().get(&asker).cloned());
    if let Some(answer) = answer {
        answer(key);
    }
}

fn transport_category(buttons: &[Button<'_>]) -> Retained<UNNotificationCategory> {
    let actions: Vec<Retained<UNNotificationAction>> = buttons
        .iter()
        .map(|button| {
            UNNotificationAction::actionWithIdentifier_title_options(
                &NSString::from_str(button.key),
                &NSString::from_str(button.label),
                UNNotificationActionOptions::empty(),
            )
        })
        .collect();

    UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
        &NSString::from_str(TRANSPORT_CATEGORY),
        &NSArray::from_retained_slice(&actions),
        &NSArray::new(),
        UNNotificationCategoryOptions::empty(),
    )
}

fn attachment_of(picture: &Path) -> Option<Retained<UNNotificationAttachment>> {
    let handed = handed_copy_of(picture)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(handed.to_str()?));

    let made = unsafe {
        UNNotificationAttachment::attachmentWithIdentifier_URL_options_error(
            &NSString::from_str("cover"),
            &url,
            None,
        )
    };
    match made {
        Ok(attachment) => Some(attachment),
        Err(_) => {
            let _ = fs::remove_file(&handed);
            None
        }
    }
}

fn handed_copy_of(picture: &Path) -> Option<PathBuf> {
    let extension = picture.extension()?.to_str()?;
    let numbered = NEXT_PICTURE.fetch_add(1, Ordering::Relaxed);
    let handed = std::env::temp_dir().join(format!(
        "resonate-notice-{}-{numbered}.{extension}",
        process::id()
    ));

    fs::copy(picture, &handed).ok().map(|_| handed)
}

fn answer_through(center: &UNUserNotificationCenter) {
    let answerer = Answerer::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*answerer)));
    std::mem::forget(answerer);
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ResonateNoticeAnswerer"]
    struct Answerer;

    unsafe impl NSObjectProtocol for Answerer {}

    unsafe impl UNUserNotificationCenterDelegate for Answerer {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            shown_as: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            shown_as
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            answered(
                &response.notification().request().identifier().to_string(),
                &response.actionIdentifier().to_string(),
            );
            done.call(());
        }
    }
);

impl Answerer {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notices_identifier_names_who_raised_it_and_what_kind_it_is() {
        assert_eq!(identifier_of(7, "track"), "7.track");
        assert_eq!(asker_of("7.track"), Some(7));
        assert_eq!(asker_of("track"), None);
        assert_eq!(
            asker_of("com.apple.UNNotificationDefaultActionIdentifier"),
            None
        );
    }
}
